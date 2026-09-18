# Contributing

Python 3.11+; no runtime Python dependencies. Install from the checkout with
`python -m pip install .`, then run `python -m unittest discover -s tests -v`.
Build a distributable with `python -m pip wheel --no-deps . --wheel-dir dist`.

Keep changes independently reviewable. Include a concrete failure or contract
when proposing abstractions, backend support, or policy changes. Follow AGENTS.md
and the architecture/security documents. Never use real credentials or personal
session stores as committed fixtures.

CI tests packaging and offline contracts on Linux, macOS, and Windows. It does
not run an OpenShell gateway or certify workload isolation. Record live evidence
with exact versions, commands, expected/observed outcomes, and no private data.
