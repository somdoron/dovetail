#!/usr/bin/env python3
"""Check local book links and marked complete examples (Python 3.11+)."""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
FENCES = re.compile(r'^```[^\n]*\n.*?^```\s*$', re.M | re.S)
EXAMPLES = re.compile(r'<!-- book-example: (.*?) -->\s*```dovetail\n(.*?)^```', re.M | re.S)


def anchors(path):
    content = FENCES.sub('', path.read_text())
    result = set(re.findall(r'(?:id|name)=["\']([^"\']+)', content))
    counts = {}
    for heading in re.findall(r'^#{1,6}\s+(.+?)\s*#*$', content, re.M):
        slug = re.sub(r'[^\w\- ]', '', heading.lower()).replace(' ', '-')
        count = counts.get(slug, 0)
        counts[slug] = count + 1
        result.add(slug + (f'-{count}' if count else ''))
    return result


def check_links(pages):
    errors = []
    for page in pages:
        content = FENCES.sub('', page.read_text())
        display = page.relative_to(ROOT) if page.is_relative_to(ROOT) else page
        for target in re.findall(r'!?\[[^\]\n]+\]\(([^)\n]+)\)', content):
            url = urlsplit(target.strip('<>'))
            if url.scheme or url.netloc:
                continue
            destination = (page.parent / unquote(url.path)).resolve() if url.path else page
            if not destination.exists():
                errors.append(f'{display}: missing {target}')
            elif url.fragment and destination.suffix == '.md' and unquote(url.fragment) not in anchors(destination):
                errors.append(f'{display}: missing anchor {target}')
    if errors:
        raise ValueError('\n'.join(errors))
    print(f'Local links checked in {len(pages)} documentation pages.', flush=True)


def command(workspace, *args):
    result = subprocess.run(
        ['cargo', 'run', '--quiet', '--manifest-path', str(ROOT / 'Cargo.toml'), '--', *args],
        cwd=workspace, text=True, capture_output=True, timeout=300,
    )
    if result.returncode:
        raise ValueError(f'dovetail {" ".join(args)} failed:\n{result.stdout}{result.stderr}')
    return result.stdout + result.stderr if args[0] == 'test' else result.stdout


def check_examples(pages, selected, *, check_init=True):
    examples = {}
    for page in pages:
        content = page.read_text()
        matches = list(EXAMPLES.finditer(content))
        if FENCES.sub('', content).count('<!-- book-example:') != len(matches):
            raise ValueError(f'Malformed example marker in {page}')
        for match in matches:
            metadata = json.loads(match[1])
            name = metadata['name']
            if not re.fullmatch(r'[a-z][a-z0-9]*', name) or name in examples:
                raise ValueError(f'Invalid or duplicate example name: {name}')
            if not re.search(r'^test ', match[2], re.M):
                raise ValueError(f'{name}: complete examples must include a test')
            examples[name] = (metadata, match[2])
    if not examples or (selected and set(selected) - examples.keys()):
        raise ValueError('No examples found, or unknown --example name')
    manifest = (ROOT / 'Dovetail.toml').read_text()
    projects = tomllib.loads(manifest)['project']
    project_names = {project['name'] for project in projects}
    with tempfile.TemporaryDirectory(prefix='dovetail-book-') as directory:
        workspace = Path(directory)
        for project in projects:
            relative = project.get('path', project['name'])
            destination = workspace / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copytree(
                ROOT / relative, destination,
                ignore=shutil.ignore_patterns('.dovetail', 'build', 'target'),
            )
        for name, (metadata, source) in examples.items():
            if selected and name not in selected:
                continue
            if name in project_names or any(dep not in project_names for dep in metadata['depends']):
                raise ValueError(f'{name}: project name collision or unknown dependency')
            folder = workspace / name / 'src'
            folder.mkdir(parents=True)
            (folder / 'main.dove').write_text(source)
            manifest += f'\n[[project]]\nname = "{name}"\nroot_package = "{name}"\npackages = ["."]\ndepends = {json.dumps(metadata["depends"])}\n'
        (workspace / 'Dovetail.toml').write_text(manifest)
        for name, (metadata, _) in examples.items():
            if selected and name not in selected:
                continue
            print(f'Checking example: {name}', flush=True)
            command(workspace, 'check', name)
            # Only format the extracted example, never the copied libraries.
            command(workspace, 'fmt', f'{name}/src/main.dove')
            command(workspace, 'fmt', '--check', f'{name}/src/main.dove')
            command(workspace, 'build', name)
            if not (workspace / 'build' / f'{name}.wasm').is_file():
                raise ValueError(f'{name}: build artifact missing')
            output = command(workspace, 'run', name)
            if output != metadata.get('stdout', ''):
                raise ValueError(f'{name}: unexpected stdout {output!r}')
            output = command(workspace, 'test', name)
            if not re.search(r'[1-9]\d* passed', output):
                raise ValueError(f'{name}: no passing tests reported:\n{output}')
        if not check_init:
            return
        # Exercise the documented first-user command with the generated manifest.
        fresh = workspace / 'first-user'
        fresh.mkdir()
        command(fresh, 'init', 'hello')
        command(fresh, 'fmt', '--check')
        metadata, source = examples['hello']
        (fresh / 'hello/src/main.dove').write_text(source)
        for args in [('fmt',), ('fmt', '--check'), ('check',), ('build',)]:
            command(fresh, *args)
        if command(fresh, 'run') != metadata.get('stdout', ''):
            raise ValueError('Initialized hello project produced unexpected output')
        output = command(fresh, 'test', 'hello', '--filter', 'greets', '--file', 'hello/src/main.dove')
        if not re.search(r'[1-9]\d* passed', output):
            raise ValueError(f'Initialized hello project ran no passing tests:\n{output}')
    print('Book examples and project initialization passed.', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--links-only', action='store_true')
    parser.add_argument('--example', action='append', help='Validate one named example while developing documentation')
    args = parser.parse_args()
    pages = sorted((ROOT / 'book').glob('*.md'))
    try:
        check_links([ROOT / 'README.md', *pages])
        if not args.links_only:
            check_examples(pages, args.example)
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        parser.exit(1, f'{error}\n')


if __name__ == '__main__':
    main()
