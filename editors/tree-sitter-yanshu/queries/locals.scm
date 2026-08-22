; Exact sequential let visibility and shadowing belong to yanshu-lsp's
; SymbolIndex. Deliberately do not approximate let bindings here.

(function_expression) @local.scope
(match_arm) @local.scope
(match_default_arm) @local.scope

(parameter_list parameter: (symbol) @local.definition)
(binding_pattern (symbol) @local.definition)

(variable (symbol) @local.reference)
