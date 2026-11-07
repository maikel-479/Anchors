module.exports = grammar({
  name: "comment",

  rules: {
    source: $ => repeat(choice($.tag, $._text)),
    tag: $ => seq(
      field("name", $.name),
      optional(field("user", $.user)),
      optional(":")
    ),
    name: $ => /[A-Z][A-Z0-9_]*/,
    user: $ => seq("(", /[^()]+/, ")"),
    _text: $ => /[^\n]/,
  },
});
