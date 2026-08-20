# Yanshu v0.7 language contract

Status: implemented Rust contract. Guest language version: `3`.

## Modules

Each source file remains an independently readable `(program ...)`. A module declares direct dependencies with one optional `(imports module-name ...)` form. An imported module contributes only names in its `export` form; private definitions never enter another module's scope.

An unlinked program with imports is not executable and returns `RUNTIME_UNLINKED_IMPORTS`. Execution therefore cannot silently depend on host search paths.

## User-defined data

`(data type-name (constructor field ...) ...)` declares a closed set of variants. Constructors are ordinary callable values with exact arity. Variant results use the portable JSON shape:

```json
{"$type":"module/type","$variant":"module/constructor","fields":[]}
```

A module may export definitions and constructors. Constructor, schema, and definition bindings cannot collide.

## Pattern matching

`(match value (pattern expression) ... (_ fallback))` evaluates its value once and tries arms from left to right. Patterns support scalar literals, bindings, `_`, and nested constructor patterns. Binding names must be unique in one pattern.

Every v3 match must end with `_`. v0.8 may remove that requirement when its type checker can prove exhaustiveness. Pattern nodes consume fuel.

## Sealed Bundle

`bundle.json` contains `formatVersion`, one guest `languageVersion`, the entry module, and a name-sorted list of `{name,path,sha256}` module records. Its bundle ID is the SHA-256 of the canonical compact manifest JSON.

Loading rejects:

- module hash or declared-name mismatch;
- absolute, parent-relative, backslash, symlink-escaping, or non-`.yan` paths;
- missing imports, cycles, and modules unreachable from the entry;
- duplicate names or paths and non-canonical module order;
- incompatible language/library versions;
- routes outside the entry module;
- ambiguous imported names.

The linker prefixes every private binding with its module name, resolves lexical scope separately, and creates only entry-export aliases. The linked program has no remaining imports.

## CLI

```text
yanshu-cli seal-bundle <directory> <entry> <module.yan>...
yanshu-cli inspect-bundle <directory>
yanshu-cli run-bundle <directory> <export> <arguments.json>
```

For the checked-in example:

```text
cargo run -p yanshu-cli -- seal-bundle examples/bundles/expense-approval expense-app app.yan policy.yan
cargo run -p yanshu-cli -- run-bundle examples/bundles/expense-approval evaluate examples/bundles/expense-approval/arguments.json
```

## Security invariant

Modules change composition, not authority. They add no ambient I/O, dynamic loading, `eval`, or FFI. Bundle resolution is completed and verified before guest execution. All first-party Rust remains `unsafe`-forbidden.
