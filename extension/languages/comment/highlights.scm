; Tag-only styling: only the anchor (name + user) is highlighted.
; The comment body stays the theme's normal comment color so inline
; **bold** / *italic* / `code` spans from the LSP remain visible.

((tag
  (name) @constant.comment.todo
  (user)? @constant.comment.todo.user)
  (#match? @constant.comment.todo "^(TODO)$"))

((tag
  (name) @string.comment.info
  (user)? @string.comment.info.user)
  (#match? @string.comment.info "^(NOTE)$"))

((tag
  (name) @property.comment.error
  (user)? @property.comment.error.user)
  (#match? @property.comment.error "^(FIXME|XXX|BUG)$"))

((tag
  (name) @keyword.comment.warn
  (user)? @keyword.comment.warn.user)
  (#match? @keyword.comment.warn "^(HACK)$"))
