# Security Policy

## Supported versions

Mind Place is pre-1.0. Security fixes are applied to the latest release line.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting for this repository when available. Do not include private conversation data, archive contents, tokens, credentials, or other personal information in a public issue.

For ordinary bugs that do not expose sensitive information, open a regular GitHub issue with the smallest reproducible example possible.

## Security model

Mind Place stores archives locally, renders saved content through a restricted desktop Content Security Policy, sanitizes rendered Markdown/HTML, and uses a browser extension with minimal permissions. Automatic capture requires explicit opt-in to ChatGPT origin access and does not scroll or load additional history.
