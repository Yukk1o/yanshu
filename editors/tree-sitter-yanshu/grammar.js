/**
 * @file Display-only incremental grammar for the Yanshu language
 * @license MIT OR Apache-2.0
 */

// Tree-sitter is an editor-facing, error-tolerant parser. The safe-Rust Reader
// and Parser remain authoritative for execution, version gates, hashes, and
// capability analysis.

const WHITESPACE = /[\s\u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]+/;

function delimited(contents) {
  return choice(
    seq('(', contents, ')'),
    seq('[', contents, ']'),
    seq('{', contents, '}'),
  );
}

module.exports = grammar({
  name: 'yanshu',

  extras: $ => [
    WHITESPACE,
    $.comment,
  ],

  word: $ => $.symbol,

  rules: {
    source_file: $ => optional($.program),

    program: $ => delimited(seq(
      'program',
      repeat($._program_member),
    )),

    _program_member: $ => choice(
      $.name_form,
      $.version_form,
      $.capabilities_form,
      $.libraries_form,
      $.imports_form,
      $.data_form,
      $.export_types_form,
      $.signature_form,
      $.schema_form,
      $.route_form,
      $.definition_form,
      $.export_form,
    ),

    name_form: $ => delimited(seq(
      'name',
      field('name', $.symbol),
    )),

    version_form: $ => delimited(seq(
      'version',
      field('version', $.integer),
    )),

    capabilities_form: $ => delimited(seq(
      'capabilities',
      repeat(field('capability', $.symbol)),
    )),

    libraries_form: $ => delimited(seq(
      'libraries',
      repeat($.library_requirement),
    )),

    library_requirement: $ => delimited(seq(
      field('name', $.symbol),
      field('version', $.integer),
    )),

    imports_form: $ => delimited(seq(
      'imports',
      repeat(field('module', $.symbol)),
    )),

    data_form: $ => delimited(seq(
      'data',
      field('name', $.symbol),
      repeat1($.data_variant),
    )),

    data_variant: $ => delimited(seq(
      field('name', $.symbol),
      repeat(field('field', $._data_field)),
    )),

    _data_field: $ => choice(
      $.symbol,
      $.typed_data_field,
    ),

    typed_data_field: $ => delimited(seq(
      field('name', $.symbol),
      field('type', $._type_expression),
    )),

    export_types_form: $ => delimited(seq(
      'export-types',
      repeat(field('type', $.symbol)),
    )),

    signature_form: $ => delimited(seq(
      'signature',
      field('name', $.symbol),
      field('type', $.function_type),
    )),

    _type_expression: $ => choice(
      $.type_name,
      $.list_type,
      $.result_type,
      $.function_type,
    ),

    type_name: $ => $.symbol,

    list_type: $ => delimited(seq(
      'list',
      field('item', $._type_expression),
    )),

    result_type: $ => delimited(seq(
      'result',
      field('success', $._type_expression),
      field('error', $._type_expression),
    )),

    function_type: $ => delimited(seq(
      'fn',
      field('parameters', $.type_parameter_list),
      field('result', $._type_expression),
    )),

    type_parameter_list: $ => delimited(repeat($._type_expression)),

    schema_form: $ => delimited(seq(
      'schema',
      field('name', $.symbol),
      field('specification', $._schema_specification),
    )),

    _schema_specification: $ => choice(
      $.any_schema,
      $.boolean_schema,
      $.string_schema,
      $.integer_schema,
      $.enum_schema,
      $.union_schema,
      $.list_schema,
      $.object_schema,
    ),

    any_schema: $ => $.schema_any_keyword,
    boolean_schema: $ => $.schema_boolean_keyword,

    string_schema: $ => choice(
      $.schema_string_keyword,
      delimited(seq(
        $.schema_string_keyword,
        field('minimum', $.integer),
        field('maximum', $.integer),
      )),
    ),

    integer_schema: $ => choice(
      $.schema_integer_keyword,
      delimited(seq(
        $.schema_integer_keyword,
        field('minimum', $.integer),
        field('maximum', $.integer),
      )),
    ),

    enum_schema: $ => delimited(seq(
      'enum',
      field('value', $.literal),
      repeat(field('value', $.literal)),
    )),

    union_schema: $ => delimited(seq(
      'union',
      field('variant', $._schema_specification),
      field('variant', $._schema_specification),
      repeat(field('variant', $._schema_specification)),
    )),

    list_schema: $ => delimited(seq(
      $.schema_list_keyword,
      field('item', $._schema_specification),
      field('minimum', $.integer),
      field('maximum', $.integer),
    )),

    schema_any_keyword: _ => 'any',
    schema_boolean_keyword: _ => 'boolean',
    schema_string_keyword: _ => 'string',
    schema_integer_keyword: _ => 'integer',
    schema_list_keyword: _ => 'list',

    object_schema: $ => delimited(seq(
      'object',
      repeat($.schema_field),
    )),

    schema_field: $ => choice(
      $.required_schema_field,
      $.optional_schema_field,
    ),

    required_schema_field: $ => delimited(seq(
      'required',
      field('name', $.string),
      field('specification', $._schema_specification),
    )),

    optional_schema_field: $ => delimited(seq(
      'optional',
      field('name', $.string),
      field('specification', $._schema_specification),
      optional(field('default', $._datum)),
    )),

    route_form: $ => delimited(seq(
      'route',
      field('method', $.http_method),
      field('path', $.string),
      field('handler', $.symbol),
    )),

    http_method: _ => token(choice(
      /[Gg][Ee][Tt]/,
      /[Pp][Oo][Ss][Tt]/,
      /[Pp][Uu][Tt]/,
      /[Pp][Aa][Tt][Cc][Hh]/,
      /[Dd][Ee][Ll][Ee][Tt][Ee]/,
    )),

    definition_form: $ => delimited(seq(
      'def',
      field('name', $.symbol),
      field('value', $._expression),
    )),

    export_form: $ => delimited(seq(
      'export',
      repeat(field('name', $.symbol)),
    )),

    _expression: $ => choice(
      $.literal,
      $.variable,
      $.empty_list,
      $.quote_expression,
      $.if_expression,
      $.and_expression,
      $.or_expression,
      $.cond_expression,
      $.match_expression,
      $.let_expression,
      $.function_expression,
      $.do_expression,
      $.call_expression,
    ),

    variable: $ => $.symbol,

    empty_list: _ => choice(
      seq('(', ')'),
      seq('[', ']'),
      seq('{', '}'),
    ),

    quote_expression: $ => choice(
      seq("'", field('value', $._datum)),
      delimited(seq('quote', field('value', $._datum))),
    ),

    if_expression: $ => delimited(seq(
      'if',
      field('condition', $._expression),
      field('consequence', $._expression),
      field('alternative', $._expression),
    )),

    and_expression: $ => delimited(seq(
      'and',
      repeat(field('operand', $._expression)),
    )),

    or_expression: $ => delimited(seq(
      'or',
      repeat(field('operand', $._expression)),
    )),

    cond_expression: $ => delimited(seq(
      'cond',
      repeat($.cond_clause),
      $.cond_else_clause,
    )),

    cond_clause: $ => delimited(seq(
      field('condition', $._expression),
      field('value', $._expression),
    )),

    cond_else_clause: $ => delimited(seq(
      'else',
      field('value', $._expression),
    )),

    match_expression: $ => delimited(seq(
      'match',
      field('value', $._expression),
      repeat($.match_arm),
      $.match_default_arm,
    )),

    match_arm: $ => delimited(seq(
      field('pattern', $._non_default_pattern),
      field('value', $._expression),
    )),

    match_default_arm: $ => delimited(seq(
      field('pattern', $.wildcard_pattern),
      field('value', $._expression),
    )),

    _pattern: $ => choice(
      $.wildcard_pattern,
      $._non_default_pattern,
    ),

    _non_default_pattern: $ => choice(
      $.literal_pattern,
      $.binding_pattern,
      $.variant_pattern,
    ),

    wildcard_pattern: _ => '_',
    literal_pattern: $ => $.literal,
    binding_pattern: $ => $.symbol,

    variant_pattern: $ => delimited(seq(
      field('constructor', $.symbol),
      repeat(field('field', $._pattern)),
    )),

    let_expression: $ => delimited(seq(
      'let',
      field('bindings', $.let_binding_list),
      field('body', $._expression),
    )),

    let_binding_list: $ => delimited(repeat($.let_binding)),

    let_binding: $ => delimited(seq(
      field('name', $.symbol),
      field('value', $._expression),
    )),

    function_expression: $ => delimited(seq(
      'fn',
      field('parameters', $.parameter_list),
      field('body', $._expression),
    )),

    parameter_list: $ => delimited(repeat(field('parameter', $.symbol))),

    do_expression: $ => delimited(seq(
      'do',
      field('expression', $._expression),
      repeat(field('expression', $._expression)),
    )),

    call_expression: $ => delimited(seq(
      field('function', $._expression),
      repeat(field('argument', $._expression)),
    )),

    _datum: $ => choice(
      $.literal,
      $.symbol,
      $.quoted_datum,
      $.datum_list,
    ),

    quoted_datum: $ => seq("'", field('value', $._datum)),

    datum_list: $ => delimited(repeat($._datum)),

    literal: $ => choice(
      $.integer,
      $.boolean,
      $.string,
    ),

    integer: _ => token(prec(1, /[+-]?[0-9]+/)),
    boolean: _ => choice('#t', '#true', '#f', '#false'),

    string: $ => seq(
      '"',
      repeat(choice(
        $._string_content,
        $.escape_sequence,
      )),
      '"',
    ),

    _string_content: _ => token.immediate(prec(1, /[^"\\]+/)),
    escape_sequence: _ => token.immediate(/\\[abtnvfre"'\\]/),

    symbol: _ => token(prec(-1, /[^\s()\[\]{}"'#;][^\s()\[\]{}"';]*/)),

    comment: _ => token(seq(';', /[^\r\n]*/)),
  },
});
