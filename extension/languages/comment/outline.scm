(tag
  (name) @name
  (user)? @context
  (#match? @name "^(TODO|NOTE|FIXME|XXX|BUG|HACK)$")
  ) @item
