# fuel 字节码与 WASM

v0.10 把已通过类型、效果和 package lock 门禁的 v4 程序编译为确定性字节码，再封装为标准 WebAssembly 模块。编译是新的执行后端，不是绕过语言安全边界的捷径。

## 一条真实命令链

```powershell
$store = ".runtime\v0.9-package-store"
$lock = "examples\packages\typed-expense\yanshu.lock.json"

cargo run --locked -p yanshu-cli -- `
  package-compile $store $lock `
  .runtime\typed-expense.ybc.json `
  .runtime\typed-expense.wasm

cargo run --locked -p yanshu-cli -- `
  package-run-compiled $store $lock `
  .runtime\typed-expense.wasm evaluate `
  examples\packages\typed-expense\arguments.json
```

执行结果同时给出业务值和成本：

```json
{
  "execution": {
    "fuelConsumed": 67,
    "fuelLimit": 10000,
    "fuelRemaining": 9933
  },
  "result": {
    "amount": 1200,
    "reason": "manual approval required",
    "status": "review"
  }
}
```

## 为什么先有字节码 VM

Yanshu 的 `Int` 是任意精度整数，条件只有 `#f` 为假，集合、Schema、Library Backend 和 capability 都有自己的 fuel 成本。直接把它们草率映射成 WASM `i64` 和普通函数调用会改变语义。

v0.10 因此把完整语义放在一个小而可验证的栈式 VM 中。原语仍调用解释器已经验证过的同一套 Rust 实现，避免形成“解释语义”和“编译语义”两套易漂移代码。

## verifier 检查什么

执行前，verifier 会检查：

- jump 和 closure block 引用不越界；
- 每条控制流路径的 stack/scope 深度一致；
- 不会 stack/scope 下溢；
- return 恰好留下一个值且退出全部局部 scope；
- export 对应真实 definition；
- block、instruction 和名称都在硬上限内。

文件中的字节码不会被直接信任。加载器根据已经验证的源程序或 lock 重新生成规范产物，再做完整 envelope 比较；任何字节变化都会被拒绝。

## fuel 怎样计算

VM 在每个源码表达式对应的显式 `charge` 点扣 1 fuel；跳转、栈维护和 scope 不因后端实现细节额外收费。随后叠加实际经过的动态成本，例如值大小、list 元素、pattern 节点、Schema 节点和 Library Backend contract。解释器与 VM 因而具有相同的 fuel 耗尽边界。

`staticInstructionWeight` 只表示产物中所有 code block 的静态指令权重，用于容量观察。它不是运行时成本上下界；真实请求成本看 `fuelConsumed`。

## WASM ABI

生成的 `.wasm` 是标准 WebAssembly v1 模块，导入：

```text
yanshu_v1.execute
```

并导出：

```text
yanshu_format_version() -> i32
yanshu_static_instruction_weight() -> i64
yanshu_run(export_index: i32, arguments_handle: i32, fuel: i64) -> result_handle: i64
```

参数和结果用不透明 handle 穿过 ABI；`BigInt`、Map、Result 或用户数据类型都不暴露 Rust 内存布局。`yanshu_run` 把执行交给加载并验证同一模块字节码段的受信任宿主。

::: warning 当前边界
v0.10 已有可实例化 WASM ABI 与完整 fuel 字节码语义，但尚未做原生 WASM 指令级 lowering。这个边界是有意保留的：优化后端必须先证明不会破坏 BigInt、truthiness、效果和 fuel。
:::

完整语义契约见 [v0.10 规格](/source/docs/specs/v0.10.md.txt)，命令参数见 [CLI 参考](/reference/cli)。
