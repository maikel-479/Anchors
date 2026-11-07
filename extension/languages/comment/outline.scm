(tag
  (name) @name
  (
      "(" @context
      (user) @context
      ")" @context
  )?
  .
  (text)? @context
  (#match? @name "^(TODO|NOTE|FIXME|XXX|BUG|HACK)$")
  ) @item
