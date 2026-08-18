# Git Workflow

## Long-lived branches

- `main` is always a tested, releasable checkpoint. Prototype release points are
  annotated with `v*` tags.
- `develop` is the integration branch for the next release.
- `feature/<name>` branches from `develop` and contains one independently
  reviewable capability.

Current line of development:

```text
main (v0.1.0)
  └─ develop
       ├─ feature/web-backend-runtime (merged as v0.2 checkpoint)
       ├─ feature/business-backend-v0.3 (merged)
       └─ feature/library-backend-v0.4
```

## Merge policy

1. Start a feature from an up-to-date `develop`.
2. Keep commits cohesive: specification, runtime, adapter, example, and tests
   should be separable when that does not leave misleading behavior.
3. Run `scripts/test.ps1` before merging.
4. Merge features into `develop` with `--no-ff` so the capability boundary
   remains visible.
5. Merge a verified release from `develop` into `main`, then add an annotated
   version tag.

Emergency fixes branch from `main` as `hotfix/<name>`, merge back to both
`main` and `develop`, and receive a patch tag.

## Repository rules

- `.toolchains/`, `.runtime/`, `.env`, generated files, and credentials never
  enter Git.
- Content-addressed runtime candidates are evidence, not source history. A
  candidate becomes project source only through an explicit reviewed commit.
- LLM-generated changes follow the same branch, test, review, and merge rules as
  human-written changes.
- Do not rewrite shared branch history. Prefer a revert commit for an already
  published change.

