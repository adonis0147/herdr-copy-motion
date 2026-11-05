# Repository guidelines

This repository is a herdr plugin.

- Maintain project documentation under `docs/`.
- Keep `README.md` at the repository root as a human-readable introduction; link to detailed documentation in `docs/`.

- Use Conventional Commits:
  ```text
  <type>[optional scope][!]: <description>

  [optional body]

  [optional footer(s)]
  ```
  Use `feat` for features and `fix` for bug fixes; other types include `docs`, `refactor`, `test`, and `chore`. Mark breaking changes with `!` before `:` or a `BREAKING CHANGE: <description>` footer.
- Prefer clear names and straightforward code that humans can easily read.
- Keep functions short and focused on one responsibility.
- Place the entry-point function before its helpers. Order helpers depth-first: each helper, then its callees, before the next sibling helper.
