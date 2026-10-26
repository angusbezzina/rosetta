# Static Atlas site

Run `rosetta atlas site --out site` in an initialized local checkout. The output contains `index.html`, `site-viewer.js`, `atlas-viewer.css`, `bundle.json` and `brand.json`. `--brand colours.json` accepts the same optional three-colour JSON as `atlas open`. Open `site/index.html` directly or serve the directory from any static host. The page embeds its pinned bundle so browsers can open it from `file://`; the JSON files are also available to tools and hosts. Re-export after the repository or model changes.

The export is read-only. It has no local curation session, source excerpt endpoint or server-side focused-view regeneration. Evidence paths and the repository snapshot remain in the bundle; open a local checkout with `rosetta atlas open` to inspect source excerpts or accept corrections. Publish only a model and repository evidence you intend to make public.

For a repository that has installed a pinned Rosetta binary, this GitHub Pages workflow publishes the export after a push to `main`. Enable **GitHub Actions** as the Pages source first. Replace the install step with the released tag or binary pin used by that repository; the example tag is a placeholder until Rosetta's first release.

```yaml
name: Publish Atlas
on:
  push:
    branches: [main]
  workflow_dispatch:
permissions:
  contents: read
concurrency:
  group: github-pages
  cancel-in-progress: true
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v7
        with:
          persist-credentials: false
      - name: Install pinned Rosetta release
        run: cargo install --git https://github.com/angusbezzina/rosetta --tag v0.1.0 --locked --bin rosetta rosetta-repo-cli
      - name: Export Atlas
        run: rosetta atlas site --out _site
      - uses: actions/upload-pages-artifact@v4
        with:
          path: _site
  deploy:
    needs: build
    runs-on: ubuntu-latest
    permissions:
      pages: write
      id-token: write
    environment:
      name: github-pages
      url: ${{ steps.publish.outputs.page_url }}
    steps:
      - id: publish
        uses: actions/deploy-pages@v4
```

The upload and deploy steps follow [GitHub's custom Pages workflow](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages). Package download occurs before Rosetta's local analysis; the exported site itself needs no Rosetta process.
