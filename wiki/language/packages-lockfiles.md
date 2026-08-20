# 内容寻址包与锁文件

v0.9 让多 Bundle 工程可以复用包，但运行边界仍然是一个完整、可重建的依赖闭包。包管理器不会下载后“顺手执行”安装脚本，也不会在运行时沿开发目录找源码。

## 开发描述与运行制品分离

开发目录使用 `yanshu-package.source.json`：

```json
{
  "formatVersion": 1,
  "name": "typed-expense-app",
  "version": "1.0.0",
  "entry": "typed-expense",
  "modules": ["app.yan"],
  "dependencies": [
    {"name":"typed-policy-lib","path":"packages/typed-policy"}
  ]
}
```

这里的 `path` 只服务开发期打包，必须是根 workspace 内的规范相对路径。打包后，它被依赖包的 SHA-256 取代：

```text
开发目录 ──解析/验证──► store/sha256/<package-hash>/
                              ├── package.json
                              └── *.yan
```

artifact 不包含 path、安装脚本、Cargo build script、动态库或 registry URL。已有 hash 目录绝不覆盖，只重新校验。

## 锁文件锁住什么

`yanshu.lock.json` 固定：

- 根 package hash；
- 入口模块与语言版本；
- 每个 package 的名字、版本、hash 和直接依赖 hash；
- 完整链接后静态计算的 capability 闭包。

```json
{
  "formatVersion": 1,
  "rootPackage": "c33d...f5916",
  "entryModule": "typed-expense",
  "languageVersion": 4,
  "capabilityClosure": ["log"],
  "packages": ["..."]
}
```

加载器不直接相信 lock：它从 store 重新检查 package 路径 hash、manifest、每个源码 hash、模块身份、依赖身份、导入闭包、类型与效果，再重建一份规范 lock 做精确比较。运行锁文件时完全不读取开发 source path。

## 实际运行

```powershell
$store = ".runtime\v0.9-package-store"
$workspace = "examples\packages\typed-expense"

cargo run --locked -p yanshu-cli -- package-lock `
  $workspace $store "$workspace\yanshu.lock.json"

cargo run --locked -p yanshu-cli -- package-review `
  $store "$workspace\yanshu.lock.json" --text

cargo run --locked -p yanshu-cli -- package-run `
  $store "$workspace\yanshu.lock.json" evaluate "$workspace\arguments.json"
```

示例的结果为 `status = review`。开发目录随后即使发生变化，旧 lock 仍运行旧 hash；store 制品若被篡改则立即失败。

## 当前的版本冲突策略

一个 lock 闭包内，同一 package name 只能对应一个 hash。当前模块身份还没有编码 package version，因此“同时加载同名包的两个版本”会造成命名空间含义不清，v0.9 选择拒绝，而不是偷偷挑一个版本。

## 它没有扩大权限

包只组织和寻址源码，不获得 capability。依赖仍要经过 Parser、Bundle 链接规则、类型/效果分析、fuel 与宿主注入；包管理没有网络解析、安装 hook、动态加载、`eval` 或 FFI。

完整契约见 [v0.9 规格](/source/docs/spec-v0.9.md.txt)，实现见 [yanshu-package store](/source/rust/crates/yanshu-package/src/store.rs.txt)和[格式解析](/source/rust/crates/yanshu-package/src/parse.rs.txt)。
