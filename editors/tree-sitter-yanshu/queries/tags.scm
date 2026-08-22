(definition_form
  name: (symbol) @name
  value: (function_expression)) @definition.function

(data_form
  name: (symbol) @name) @definition.type

(data_variant
  name: (symbol) @name) @definition.class

(schema_form
  name: (symbol) @name) @definition.type
