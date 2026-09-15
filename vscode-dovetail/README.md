# Dovetail for VS Code

The extension uses `dovetail lsp-server` for language support. Set
`dovetail.serverPath` if the compiler is not on your PATH.

## Formatting

Choose **Format Document** to format the current buffer, including unsaved
changes. Formatting uses the same canonical style as `dovetail fmt`: four
spaces and a 100-column target. Editor indentation settings do not change this
style. Syntax errors leave the document unchanged.

To select Dovetail as your formatter and enable formatting on save, add:

```json
{
  "[dovetail]": {
    "editor.defaultFormatter": "dovetail-lang.dovetail-language",
    "editor.formatOnSave": true
  }
}
```

Format the local workspace from a terminal with `dovetail fmt`, or use
`dovetail fmt --check` in CI. Explicit `.dove` file arguments work without a
workspace manifest. Formatting does not fetch or format dependencies.
