# Git Workflow

Status: current contributor workflow.

## Active branches

- `main` is the only release source and must remain a tested checkpoint.
- `feature/<name>` branches from an up-to-date `main` and contains one
  independently reviewable capability.
- `release/<name>` preserves an important historical checkpoint when useful;
  it is not a second publishing source.
- `hotfix/<name>` branches from `main` and returns through a reviewed pull
  request.

The old `develop` branch is retained as history but is no longer the active
integration branch. Current work uses short-lived feature branches and pull
requests against `main` so branch policy and release provenance have one root.

## Merge policy

1. Start a feature from an up-to-date `main`.
2. Keep commits cohesive: specification, runtime, adapter, example, and tests
   should be separable when that does not leave misleading behavior.
3. Run `scripts/test.ps1` before merging.
4. Merge the reviewed pull request into `main` without rewriting shared history.
5. After every release gate passes, create an annotated tag whose name exactly
   matches the workspace version. The tag-only workflow verifies it is contained
   in `main` before publishing.

Emergency fixes branch from `main` as `hotfix/<name>`, merge back to `main`, and
receive a matching patch tag only after the normal gates pass.

## Repository rules

- `.toolchains/`, `.runtime/`, `.env`, generated files, and credentials never
  enter Git.
- Content-addressed runtime candidates are evidence, not source history. A
  candidate becomes project source only through an explicit reviewed commit.
- LLM-generated changes follow the same branch, test, review, and merge rules as
  human-written changes.
- Do not rewrite shared branch history. Prefer a revert commit for an already
  published change.
- Do not publish or replace release assets by hand. The release workflow,
  checksums, SBOM, and keyless provenance form one reviewed evidence chain.
