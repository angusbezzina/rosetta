# Rosetta in a pull request

The root [composite action](../action.yml) runs the local binary against a checked-out repository. It checks generated glossary and Atlas output, scans changed copy for settled Avoid words, and emits SARIF plus GitHub check annotations. Findings keep the job green by default; set `strict: 'true'` when the team is ready to enforce them. A missing binary or an analysis error still fails the job.

After the first Rosetta release, add this workflow to the repository being checked:

```yaml
name: Rosetta
on: pull_request
permissions:
  contents: read
  security-events: write # needed for code scanning SARIF upload
jobs:
  vocabulary:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          fetch-depth: 0 # changed-file lint uses the PR base commit
          persist-credentials: false
      - uses: angusbezzina/rosetta@v0.1.0 # use a published tag or commit SHA
        with:
          version: v0.1.0
          strict: 'false'
          commands: glossary-check,glossary-lint,atlas-check
```

The action selects a release asset for macOS arm64/x64, Linux arm64/x64, or Windows x64, checks its SHA-256 digest, and starts `rosetta`. The release contract is `rosetta-<target-triple>.tar.gz` with a matching `.sha256` file. The first release has not been published yet; the tag above is an example until that happens. For a pre-release workflow that builds Rosetta itself, set `binary-path` to the absolute executable path instead of downloading a release.

`commands` is a comma-separated subset of `glossary-check`, `glossary-lint`, and `atlas-check`. The first and third check source and generated files; lint scans changed tracked copy against the pull request base SHA. Use `fetch-depth: 0` so the base is present. If it is unavailable, the action states that it is scanning all tracked copy. `strict: 'true'` fails the job after SARIF upload when any selected check reports a finding. The default warning mode emits annotations and succeeds when findings exist.

GitHub code scanning shows SARIF results inline on changed lines when the repository supports code scanning and the workflow has `security-events: write`. If upload is unavailable, the action's problem matcher still emits check annotations from the same findings. GitHub documents [SARIF upload and permissions](https://docs.github.com/en/code-security/how-tos/find-and-fix-code-vulnerabilities/integrate-with-existing-tools/upload-sarif-file) and [composite actions](https://docs.github.com/en/actions/reference/workflows-and-actions/metadata-syntax).
