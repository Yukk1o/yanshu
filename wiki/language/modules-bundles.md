# 模块、数据类型与密封 Bundle

v3 把“单文件可审计”扩展成“依赖闭包可审计”：源码可以拆成模块，但运行制品必须先收敛成一个内容寻址、完整验证的 Bundle。模块不会变成动态加载或环境搜索路径。

## 两个模块怎样协作

策略模块定义自己的数据和函数：

```lisp
(program
  (name policy)
  (version 3)
  (data decision
    (approved amount)
    (review amount reason))
  (def decide ...)
  (export decide approved review))
```

入口模块只导入公开接口：

```lisp
(program
  (name expense-app)
  (version 3)
  (imports policy)
  (def evaluate
    (fn (amount)
      (match (decide amount)
        ((approved value) value)
        ((review value reason) reason)
        (_ "invalid"))))
  (export evaluate))
```

完整源码在[费用审批 Bundle](/source/examples/bundles/expense-approval/app.yan.txt)和[策略模块](/source/examples/bundles/expense-approval/policy.yan.txt)。

## Bundle 是执行边界

`bundle.json` 固定入口、语言版本和每个模块的 SHA-256。v3 使用 format 1：

```json
{
  "formatVersion": 1,
  "languageVersion": 3,
  "entry": "expense-app",
  "modules": [
    {"name":"expense-app","path":"app.yan","sha256":"..."},
    {"name":"policy","path":"policy.yan","sha256":"..."}
  ]
}
```

模块必须按名称排序。规范化 manifest 的 SHA-256 就是 Bundle ID；任一源码字节、路径、入口或依赖集合改变，ID 都随之改变。

v4 使用 format 2，并增加由分析器计算的字段：

```json
{"formatVersion":2,"languageVersion":4,"capabilityClosure":["log"]}
```

加载时会重新计算并比对 closure；format 1 不能承载 v4，format 2 也不能伪装成旧语言版本。

```text
module source ─► module SHA-256 ─┐
module source ─► module SHA-256 ─┼─► canonical manifest ─► Bundle SHA-256
entry + language version ────────┘
```

## 加载时验证什么

加载器在解释任何 guest 表达式前验证：

- manifest 字段精确、模块唯一且有序；
- 路径是规范化的相对 `.yan` 路径，解析后仍在 Bundle 目录内；
- 实际源码 hash、program name 和 language version 与 manifest 一致；
- imports 全部存在、无环，并且每个模块都可从入口到达；
- 只有入口可以声明 route；
- imported export 不与本地或另一个依赖的可见名字冲突。
- v4 导入签名中的名义类型由直接依赖显式 `export-types`，且来源唯一。

失败会返回稳定诊断，不会退回按磁盘现状“尽力运行”。

## 链接不会扩大权限

链接器按依赖顺序合并 AST，把模块私有 binding、Schema、类型和构造器改写到独立命名空间，只为入口 exports 建立外部别名。值的 `export` 与类型的 `export-types` 是独立白名单；链接后的 `Program.imports` 必须为空，解释器拒绝任何未链接 imports。

capability 仍由源码声明、宿主注入。模块不能获得文件系统、网络、动态库、`eval` 或安装脚本；拆文件只改变组织方式，不改变信任模型。

## 运行示例

```powershell
cargo run --locked -p yanshu-cli -- inspect-bundle examples\bundles\expense-approval
cargo run --locked -p yanshu-cli -- run-bundle examples\bundles\expense-approval evaluate examples\bundles\expense-approval\arguments.json
```

示例结果是 `status = review`，并同时返回金额与人工审批原因。实现入口见 [yanshu-bundle manifest](/source/rust/crates/yanshu-bundle/src/manifest.rs.txt)、[依赖图](/source/rust/crates/yanshu-bundle/src/graph.rs.txt)和[链接器](/source/rust/crates/yanshu-bundle/src/linker.rs.txt)。

需要跨 Bundle/工程复用时，继续阅读[内容寻址包与锁文件](/language/packages-lockfiles)。package 复用 Bundle 的链接与类型/效果门禁，但额外锁住整个 package 依赖闭包。
