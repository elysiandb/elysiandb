package hook

func GetDefaultHookScriptJSForPostRead() string {
	return `
function postRead(ctx) {
  const entity = ctx.entity

  /*const others = ctx.query("order", {
    totoId: { eq: entity.id }
  })*/

  return entity
}
`
}
