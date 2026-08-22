(comment) @comment

(string) @string
(escape_sequence) @string.escape
(integer) @number
(boolean) @boolean

[
  "program"
  "name"
  "version"
  "capabilities"
  "libraries"
  "imports"
  "data"
  "export-types"
  "signature"
  "schema"
  "route"
  "def"
  "export"
] @keyword

[
  "quote"
  "if"
  "and"
  "or"
  "cond"
  "else"
  "match"
  "let"
  "fn"
  "do"
] @keyword.control

[
  "enum"
  "union"
  "object"
  "required"
  "optional"
  "result"
] @keyword.type

(name_form name: (symbol) @module)
(imports_form module: (symbol) @module)
(library_requirement name: (symbol) @module)

(data_form name: (symbol) @type.definition)
(schema_form name: (symbol) @type.definition)
(data_variant name: (symbol) @constructor)
(typed_data_field name: (symbol) @property)
(type_name (symbol) @type)

[
  (schema_any_keyword)
  (schema_boolean_keyword)
  (schema_string_keyword)
  (schema_integer_keyword)
  (schema_list_keyword)
] @type.builtin

(definition_form name: (symbol) @variable.definition)
(signature_form name: (symbol) @variable.definition)
(parameter_list parameter: (symbol) @variable.parameter)
(let_binding name: (symbol) @variable)
(binding_pattern (symbol) @variable.parameter)

(capabilities_form capability: (symbol) @variable.builtin)
(http_method) @constant.builtin

(call_expression
  function: (variable (symbol) @function.call))

((call_expression
   function: (variable (symbol) @operator))
 (#match? @operator "^(\\+|-|\\*|=|<|<=|>|>=)$"))

((call_expression
   function: (variable (symbol) @function.builtin))
 (#match? @function.builtin "^(quotient|remainder|checked-quotient|checked-remainder|not|integer\\?|boolean\\?|string\\?|list\\?|map\\?|empty\\?|length|first|rest|list-map|list-filter|list-fold|sum|get|get-or|has-key\\?|assoc|string-append|number->string|ok|err|ok\\?|err\\?|result-value|unwrap|validate|validate-report|api-response|api-error|log|now-ms|kv-get|kv-put|kv-delete|kv-list|map|list|text/[A-Za-z0-9?+*/<>=_-]+)$"))
