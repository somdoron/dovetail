#!/usr/bin/env python3
"""Validate bundled/installed AI guidance and execute its complete examples."""
import argparse
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import tomllib

sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
BUNDLE = ROOT / 'dovetail/ai'
spec = importlib.util.spec_from_file_location('check_book', ROOT / 'tools/check-book.py')
book = importlib.util.module_from_spec(spec)
spec.loader.exec_module(book)


def check_skill(directory, *, installed=False):
    source = (directory / 'SKILL.md').read_text()
    if not source.startswith('---\n'):
        raise ValueError(f'{directory}: missing skill frontmatter')
    frontmatter, body = source[4:].split('\n---\n', 1)
    if not re.search(r'^name: dovetail$', frontmatter, re.M):
        raise ValueError('Skill name must be dovetail')
    if not re.search(r'^description: .+', frontmatter, re.M):
        raise ValueError('Missing discovery description')
    if len(body.splitlines()) > 150:
        raise ValueError('Keep the skill entry point below 150 lines')
    pages = sorted(directory.rglob('*.md'))
    book.check_links(pages)
    linked = set(re.findall(r'\]\((references/[^)]+|reviews/[^)]+)\)', body))
    required = {str(page.relative_to(directory)) for page in pages
                if page.parent.name in ('references', 'reviews')}
    if not required <= linked:
        raise ValueError(f'Unrouted references: {required - linked}')
    if installed:
        expected = tomllib.loads((ROOT / 'dovetail/Cargo.toml').read_text())['package']['version']
        receipt = json.loads((directory / '.dovetail-install.json').read_text())
        if receipt['compiler_version'] != expected:
            raise ValueError('Installed metadata version does not match compiler')
        for path in directory.rglob('*'):
            if path.is_file() and '{{DOVETAIL_VERSION}}' in path.read_text():
                raise ValueError(f'Unrendered version in {path}')


def check_coverage():
    coverage = json.loads((ROOT / 'tools/ai-coverage.json').read_text())
    expected = {str(path.relative_to(ROOT)) for path in (ROOT / 'book').glob('[0-9]*.md')}
    actual = {entry['source'] for entry in coverage}
    if not expected <= actual:
        raise ValueError(f'Book chapters missing from AI coverage: {expected - actual}')
    for entry in coverage:
        source = ROOT / entry['source']
        headings = re.findall(r'^## (.+)$', source.read_text(), re.M)
        relevant = {heading for heading in headings if not heading.startswith('Summary')}
        if relevant != set(entry['sections']):
            raise ValueError(f'Update AI coverage for changed sections: {entry["source"]}')
        for target in entry['references'] + entry['reviews']:
            if not (BUNDLE / target).is_file():
                raise ValueError(f'Missing AI coverage destination: {target}')
    print('AI source coverage checked.', flush=True)


def check_packaging():
    result = subprocess.run(
        ['cargo', 'package', '--list', '-p', 'dovetail-lang', '--allow-dirty', '--offline'],
        cwd=ROOT, text=True, capture_output=True, timeout=300,
    )
    if result.returncode:
        raise ValueError(f'Cannot inspect Cargo package: {result.stderr}')
    packaged = set(result.stdout.splitlines())
    expected = {str(path.relative_to(ROOT / 'dovetail'))
                for path in BUNDLE.rglob('*') if path.is_file()}
    if not expected <= packaged:
        raise ValueError(f'AI assets missing from Cargo package: {expected - packaged}')
    print('All AI assets are included in the dovetail-lang Cargo package.', flush=True)


def check_installation():
    with tempfile.TemporaryDirectory(prefix='dovetail-ai-') as directory:
        workspace = Path(directory)
        book.command(workspace, 'init', 'app', '--ai', 'generic,claude')
        for target in ('.agents', '.claude'):
            check_skill(workspace / target / 'skills/dovetail', installed=True)
        for name in ('dovetail-reviewer', 'dovetail-ddd-reviewer'):
            wrapper = workspace / '.claude/agents' / f'{name}.md'
            text = wrapper.read_text()
            if f'name: {name}\n' not in text:
                raise ValueError(f'Invalid native reviewer: {name}')
            for target in re.findall(r'`(\.claude/[^`]+)`', text):
                if not (workspace / target).is_file():
                    raise ValueError(f'Broken native reviewer reference: {target}')
        book.command(workspace / 'app/src', 'ai', 'install')
        for target in ('.agents', '.claude'):
            discovery = workspace / target / 'skills/dovetail/references/api-discovery.md'
            if not discovery.is_file():
                raise ValueError(f'Missing installed API discovery reference: {target}')
        output = book.command(workspace, 'query', 'definition', 'standard.prelude.Array')
        if 'module Array<T>' not in output or 'Compiler-built-in type Array<T>' not in output:
            raise ValueError('Prelude query must return both builtin and module declarations')
        output = book.command(workspace, 'query', 'search', 'main', '--package', 'app')
        if 'app.main' not in output:
            raise ValueError('Query did not discover the initialized application')
    print('Installed generic and Claude guidance checked.', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--links-only', action='store_true', help='Check content without building/installing')
    parser.add_argument('--example', action='append', help='Run selected complete examples')
    args = parser.parse_args()
    try:
        check_skill(BUNDLE)
        check_coverage()
        if not args.links_only:
            check_packaging()
            check_installation()
            book.check_examples(sorted(BUNDLE.rglob('*.md')), args.example, check_init=False)
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
