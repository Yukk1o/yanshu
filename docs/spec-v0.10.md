# AI-Evolve v0.10：fuel 字节码与 WASM ABI

状态：已实现。本文只规定编译产物与执行边界；语言源码语义仍由 v4 AST、v0.8 类型/效果契约和 v0.9 package lock 共同定义。

## 1. 目标

v0.10 增加两个确定性编译目标：

1. `ail-bytecode-v1`：经过验证的栈式字节码，由 Rust `ail-runtime` VM 执行；
2. `ail-wasm-bytecode-v1`：标准 WebAssembly 模块，携带同一份内容寻址字节码，并通过稳定的受信任宿主 ABI 执行。

编译不能绕过 Parser、链接器、类型/效果分析、capability 闭包、Library Backend contract、export 输入/输出类型或运行时资源预算。

## 2. 编译前置条件

`compile_bytecode` 只接受：

- language version 4；
- 已完成链接、`imports` 为空的 `Program`；
- 通过 `analyze_program` 的类型与效果检查；
- export 均有签名，capability 使用不超出声明。

程序语义指纹是 `Program::inspect_json()` 的 SHA-256，不依赖开发路径、文件时间或哈希表迭代顺序。编译产物另有自己的 SHA-256。

## 3. 字节码模型

每个顶层 definition 与匿名函数对应一个 `CodeBlock`。指令集只包含：

- 常量、名字读取、栈丢弃；
- 无条件跳转、按 AIL truthiness 跳转、保留操作数的短路跳转；
- 词法 scope 进入、绑定和退出；
- 闭包创建与调用；
- pattern 尝试、match 失败；
- 返回。

原语、Schema、capability 与 Library Backend 不在编译器里复制实现。VM 调用与解释器相同的 Rust 运行时路径，因此保留：

- 任意精度 `BigInt`；
- 只有 `Bool(false)` 为假的 truthiness；
- 从左到右求值和 `let` 顺序绑定；
- Result、用户数据类型、pattern binding；
- Schema 逐节点成本；
- 集合逐项成本；
- Library contract 的调用前计费与返回值边界；
- capability host 的显式注入。

## 4. 字节码验证器

执行前必须运行独立 verifier。它拒绝：

- 超量 block、instruction 或名称；
- 未知 code block 和缺失 export definition；
- 越界 jump；
- stack/scope 上溢或下溢；
- 控制流汇合处不一致的 stack/scope 状态；
- fallthrough、非法 return 状态和无 return 路径。

Rust API 不公开可变 artifact 字段。文件加载器不会信任文件中的指令：它以调用方提供的已验证 `Program` 重新生成规范产物，并要求整个 envelope 完全一致。源码、lock 或 artifact 任一变化都会导致内容哈希变化或 `BYTECODE_ARTIFACT_MISMATCH`。

## 5. fuel 模型

编译器为每个源码表达式生成一个显式 `charge` 指令并扣 1 fuel；跳转、栈维护和 scope 等后端实现细节不单独收费。这样解释器与 VM 在相同程序、参数和宿主下具有相同的 fuel 耗尽边界。之后仍叠加共享运行时的动态成本：

- function/primitive 调用；
- 变量、常量、参数与返回值的节点数、标量字节数和整数位数；
- BigInt 乘除与十进制转换的保守复杂度成本；
- pattern 节点；
- list map/filter/fold/sum 的每个元素；
- Schema 节点；
- Library contract 计算出的调用成本与返回值规范化成本。

fuel 不足在执行副作用或 Library Backend 之前失败。正常执行报告包含：

```json
{
  "fuelLimit": 10000,
  "fuelConsumed": 67,
  "fuelRemaining": 9933
}
```

`staticInstructionWeight` 是所有 code block 的静态指令权重总和，只用于产物比较和容量观察，不是假装成某次执行的最低或最高 fuel。

## 6. WASM 目标

输出以标准 `\0asm` magic 和 WebAssembly version 1 开头，可被标准引擎验证和实例化。模块包含：

- import：`ail_v1.execute`；
- export：`ail_format_version() -> i32`；
- export：`ail_static_instruction_weight() -> i64`；
- export：`ail_run(export_index: i32, arguments_handle: i32, fuel: i64) -> result_handle: i64`；
- custom section：`ail.meta.v1`；
- custom section：`ail.bytecode.v1`。

`ail_run` 把 export 索引、不透明参数句柄和 fuel 传给受信任宿主。宿主必须从同一模块的 `ail.bytecode.v1` 加载并验证字节码，用显式语义计费点执行，再返回不透明结果句柄。

这是显式的 handle ABI，不暴露 Rust enum 布局，也不把 `BigInt` 偷换成 `i64`。v0.10 没有声称完成原生 WASM 指令级 lowering；未来可在保持 ABI、类型/效果和 fuel 语义的前提下增加经证明等价的优化后端。

## 7. CLI

单文件：

```text
compile-bytecode <program.ail> <artifact.aibc.json>
inspect-bytecode <program.ail> <artifact.aibc.json>
run-bytecode <program.ail> <artifact.aibc.json> <export> <arguments.json>
compile-wasm <program.ail> <artifact.wasm>
inspect-wasm <program.ail> <artifact.wasm>
run-wasm <program.ail> <artifact.wasm> <export> <arguments.json>
```

内容寻址 package：

```text
package-compile <store> <ail.lock.json> <artifact.aibc.json> <artifact.wasm>
package-run-compiled <store> <ail.lock.json> <artifact.wasm> <export> <arguments.json>
```

密封 Bundle：

```text
compile-bundle <directory> <artifact.aibc.json> <artifact.wasm>
run-bundle-compiled <directory> <artifact.wasm> <export> <arguments.json>
```

package 编译先完整重验 store、source hash、依赖闭包、lock 和 capability closure。运行编译产物仍完整重验 lock 与 WASM/bytecode 同源关系。

## 8. 安全与限制

- 第一方 Rust 继续 `#![forbid(unsafe_code)]`，编译器和 VM 不使用 `unsafe`、动态库或 `extern "C"`。
- WASM import 只是显式宿主能力边界，不允许 guest 自己选择 provider。
- v0.10 不增加通用 IO、线程、共享内存、WASI 或运行时下载。
- WASM artifact 不是独立可信来源；必须与已验证 Program/package lock 一起加载。
- 当前 VM 在宿主进程内运行。独立进程内存/CPU/墙钟配额仍属于生产强化路线。
- Reader 在 BigInt 解析前限制源码、token 与字符串；运行时统一限制 portable value 的深度、节点、标量字节与整数位数。
- `string-append`、BigInt 运算和 `text/replace` 在分配或高成本计算前预检结果上限；capability 与 Library Backend 返回值重新进入同一有界数据包络。
- 解释闭包体与编译 block 每次执行期间最多缓存一份，递归调用不会按近零 fuel 反复深拷贝程序结构。

## 9. 验收

实现必须至少通过：

- 编译器 canonical/tamper/verifier 测试；
- 解释器与字节码 VM 的 typed control-flow、match、递归结果差分；
- 解释器与字节码 VM 完全相同的 fuel 成功/耗尽边界；
- v1–v4 的条件、集合、Result、用户数据、密封 Bundle、类型与编译路径 conformance；
- 恶意大 token、深层/超大值、字符串替换放大、BigInt 和 capability 返回值的资源边界测试；
- 真实 typed-expense package 的解释执行/编译执行结果一致；
- 标准 JavaScript `WebAssembly.validate` 与实例化 smoke test；
- Codex/Claude Code 自动入口与共享代理契约不存在互相冲突的安全或验收规则；
- 全 workspace test、clippy、fmt、unsafe scan、依赖审计和 Wiki build。

## 10. Coding Agent 宿主接入

v0.10 的候选生成层可选择 `codex-cli`、`claude-code-cli` 或 `opencode-cli`。适配器必须使用非交互结构化参数而不是 shell 拼接，只向一次性目录复制当前候选和结构化观察，限制墙钟与输出文件，过滤宿主敏感环境变量，并在 agent 退出后重新走可信 Parser、suite、内容哈希和版本登记。agent 没有晋升权，候选目录也不被宣称为独立 OS 沙箱。
