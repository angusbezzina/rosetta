# Offline local core

After the Rosetta binary is installed, local repository discovery, `init`, derivation, glossary and Atlas checks, context selection, draft request and answer validation, Markdown rendering, bundle generation, and the loopback viewer use local files and Git history. They require no account, telemetry endpoint, or model API key. A draft request is a file for a person or their existing agent; Rosetta does not call a model.

`rosetta init` accepts a local Git checkout. A public repository URL is deliberately rejected; remote acquisition is deferred. `rosetta atlas open` binds to `127.0.0.1` and serves local assets and evidence. The viewer fetches only same-origin files and endpoints from that loopback server. `rosetta atlas site --out <dir>` writes a read-only snapshot that opens from a file URL or static host without a Rosetta process.

Downloading or updating an installation package can use the network. External adapters are separate from this promise: a future `gh` pull-request reader and a tracker command may contact their services only when invoked or configured explicitly. The parsed `tracker.command` setting is currently reserved and does not run. In particular, `glossary check` does not discover and run `virgil` from `PATH`. An embedding host that chooses the legacy `VirgilTasksOnPath` library adapter owns its network behavior.

The representative CLI smoke workflow is `scripts/offline-smoke.sh /absolute/path/to/rosetta`. Run it inside an OS network-denial boundary on Linux and macOS; it does not create that boundary itself. Set `ROSETTA_TEST_LOOPBACK=1` for a separate run that checks the local viewer server with loopback permitted. Compilation and package acquisition occur before entering the denied environment.
