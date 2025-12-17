package hook

import (
	"fmt"

	"github.com/dop251/goja"
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
)

func applyScript(
	script string,
	fnName string,
	entity map[string]any,
	bypassAcl bool,
) error {
	vm := goja.New()

	ctx := map[string]any{
		"entity": entity,
		"query": func(call goja.FunctionCall) goja.Value {
			if len(call.Arguments) < 2 {
				panic(vm.ToValue("query(entity, params) expected"))
			}

			targetEntity := call.Arguments[0].String()
			params, _ := call.Arguments[1].Export().(map[string]any)

			filters := make(map[string]map[string]string, len(params))
			for field, cond := range params {
				typed, ok := cond.(map[string]any)
				if !ok {
					continue
				}

				condMap := make(map[string]string, len(typed))
				for op, val := range typed {
					condMap[op] = fmt.Sprintf("%v", val)
				}

				filters[field] = condMap
			}

			results := api_storage.ListEntities(
				targetEntity,
				0,
				0,
				"",
				true,
				filters,
				"",
				"",
			)

			if !bypassAcl {
				results = acl.FilterListOfEntities(targetEntity, results)
			}

			return vm.ToValue(results)
		},
	}

	if err := vm.Set("ctx", ctx); err != nil {
		return err
	}

	if _, err := vm.RunString(script); err != nil {
		return err
	}

	fn, ok := goja.AssertFunction(vm.Get(fnName))
	if !ok {
		return nil
	}

	_, err := fn(goja.Undefined(), vm.Get("ctx"))

	return err
}

func ApplyPostReadScript(
	script string,
	entity map[string]any,
	bypassAcl bool,
) error {
	return applyScript(script, "postRead", entity, bypassAcl)
}

func ApplyPreReadScript(
	script string,
	entity map[string]any,
	bypassAcl bool,
) error {
	return applyScript(script, "preRead", entity, bypassAcl)
}
