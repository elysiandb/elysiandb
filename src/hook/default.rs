pub fn get_default_hook_script_js_for_post_read() -> String {
    "
function postRead(ctx) {
  const entity = ctx.entity

  /*const others = ctx.query(\"order\", {
    totoId: { eq: entity.id }
  })*/

  return entity
}
"
    .to_string()
}

pub fn get_default_hook_script_js_for_pre_read() -> String {
    "
function preRead(ctx) {
  const entity = ctx.entity

  /*const others = ctx.query(\"order\", {
    totoId: { eq: entity.id }
  })*/

  return entity
}
"
    .to_string()
}
