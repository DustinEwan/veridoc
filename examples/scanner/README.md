# scanner: the veridoc example project

A small TypeScript project whose documentation is proved by the build: one scenario,
an artefact claim on a byte-pinned fixture, and a citation claim on a session log.

## Setup

1. Install the plugin once per machine:

   ```sh
   veridoc install-plugin <veridoc-plugin-ts git-url>
   ```

2. Link the authoring packages from the installed checkout (they are not vendored here):

   ```sh
   tools/link-authoring.sh
   ```

   The script reads `VERIDOC_HOME` (or the XDG default) exactly like `veridoc` does, so
   point it at the same registry you installed into.

## Run

```sh
veridoc check --project .
```

Exit 0 means every claim passed its gate or its judge, the scenario ran, and the
coverage figure met `coverage.required`. The rendered documentation lands in
`.veridoc/out/`.

The judge here is `tools/scripted-judge.mjs`, a fixture that answers the pinned brief
hashes deterministically; a real project sets `judge.transport` to `"anthropic"` or
points `"command"` at its own model runner.
