# Dockerfile Authoring

> Trigger: new Dockerfile / docker workflow / `FROM` change.
> SSOT: `platform-gitops` mirror + overlay system. This repo never references it.

## Rule of Separation

App-side concerns (this repo) vs platform-side (platform-gitops):

| Concern | Owner | This repo |
| ------- | ----- | --------- |
| Upstream image to use (`rust:1-alpine3.23`) | app | ✓ |
| Upstream → mirror redirect (`gitea.girok.dev/...`) | platform | ✗ |
| `cargo`/`npm`/`pip` config baked into mirror | platform | ✗ |
| Cache host (`cargo.girok.dev` etc) | platform | ✗ |

## Rules

| Allowed | Forbidden |
| ------- | --------- |
| `FROM rust:1-alpine3.23` (upstream URL) | `FROM gitea.girok.dev/mirror/...` |
| `RUN cargo build` | `ARG CARGO_REPLACE_WITH=` / `ENV CARGO_SOURCE_*=` |
| `RUN npm ci` | `ARG NPM_REGISTRY=` / `ENV NPM_CONFIG_REGISTRY=` |
| `RUN pip install` | `ENV PIP_INDEX_URL=` / `COPY pip.conf` |
| `docker buildx build` | `--build-arg CARGO_*` / `--build-arg NPM_REGISTRY=` |

## New base image needed

`rust:1.90-alpine` not in mirror? File a `platform-gitops` PR adding the tag to:

| File | Purpose |
| ---- | ------- |
| `mirrors/images/mirror-list.txt` | enables mirror pull |
| `mirrors/images/overlay-list.txt` | enables in-cluster cache wiring |

Never work around in this repo.

## Pre-PR check

```bash
git diff origin/develop -- '**/Dockerfile*' '.github/workflows/*.yml' \
  | grep -iE 'girok|cargo_replace|npm_registry|pip_index|cargo_source|npm_config_registry'
# Empty output = clean
```

## Reference

- platform-gitops `.add/add-mirror.md` — overlay add runbook
- platform-gitops `docs/llm/decisions/image-mirror-rewrite.md` — Phase 3 + 4 ADR
