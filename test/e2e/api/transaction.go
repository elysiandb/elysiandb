package e2e

import (
	"encoding/json"
	"testing"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func TestTransactions_Create_Update_Delete_Commit(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	beginResp := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var beginOut map[string]any
	_ = json.Unmarshal(beginResp.Body(), &beginOut)
	txID := beginOut["transaction_id"].(string)

	w1 := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/books", map[string]any{
		"title": "T1",
		"pages": 100,
	})
	if w1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("unexpected write1 status %d", w1.StatusCode())
	}

	w2 := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/books", map[string]any{
		"title": "T2",
		"pages": 200,
	})
	if w2.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("unexpected write2 status %d", w2.StatusCode())
	}

	listBefore := mustGET(t, client, "http://test/api/books")
	var before []map[string]any
	_ = json.Unmarshal(listBefore.Body(), &before)
	if len(before) != 0 {
		t.Fatalf("expected empty before commit, got %v", before)
	}

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("unexpected commit status %d", commit.StatusCode())
	}

	listAfter := mustGET(t, client, "http://test/api/books")
	var after []map[string]any
	_ = json.Unmarshal(listAfter.Body(), &after)
	if len(after) != 2 {
		t.Fatalf("expected 2 after commit, got %v", after)
	}
}

func TestTransactions_UpdateInsideTransaction(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	create := mustPOSTJSON(t, client, "http://test/api/articles", map[string]any{
		"title": "Old",
	})
	var created map[string]any
	_ = json.Unmarshal(create.Body(), &created)
	id := created["id"].(string)

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var out map[string]any
	_ = json.Unmarshal(begin.Body(), &out)
	txID := out["transaction_id"].(string)

	up := mustPUTJSON(t, client, "http://test/api/tx/"+txID+"/entity/articles/"+id, map[string]any{
		"title": "New",
	})
	if up.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("update failed: %d", up.StatusCode())
	}

	grBefore := mustGET(t, client, "http://test/api/articles/"+id)
	var old map[string]any
	_ = json.Unmarshal(grBefore.Body(), &old)
	if old["title"] != "Old" {
		t.Fatalf("expected Old, got %v", old["title"])
	}

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("commit failed")
	}

	grAfter := mustGET(t, client, "http://test/api/articles/"+id)
	var updated map[string]any
	_ = json.Unmarshal(grAfter.Body(), &updated)
	if updated["title"] != "New" {
		t.Fatalf("expected New, got %v", updated["title"])
	}
}

func TestTransactions_DeleteInsideTransaction(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	c1 := mustPOSTJSON(t, client, "http://test/api/items", map[string]any{"name": "A"})
	var m1 map[string]any
	_ = json.Unmarshal(c1.Body(), &m1)
	id := m1["id"].(string)

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var out map[string]any
	_ = json.Unmarshal(begin.Body(), &out)
	txID := out["transaction_id"].(string)

	del := mustDELETE(t, client, "http://test/api/tx/"+txID+"/entity/items/"+id)
	if del.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("delete failed %d", del.StatusCode())
	}

	grBefore := mustGET(t, client, "http://test/api/items/"+id)
	if string(grBefore.Body()) == "null" {
		t.Fatalf("deleted before commit")
	}

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("commit failed")
	}

	grAfter := mustGET(t, client, "http://test/api/items/"+id)
	if string(grAfter.Body()) != "null" {
		t.Fatalf("expected null, got %s", grAfter.Body())
	}
}

func TestTransactions_Rollback(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var out map[string]any
	_ = json.Unmarshal(begin.Body(), &out)
	txID := out["transaction_id"].(string)

	w := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/users", map[string]any{
		"name": "X",
	})
	if w.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("write failed")
	}

	rb := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/rollback", nil)
	if rb.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("rollback failed")
	}

	list := mustGET(t, client, "http://test/api/users")
	var arr []map[string]any
	_ = json.Unmarshal(list.Body(), &arr)
	if len(arr) != 0 {
		t.Fatalf("rollback did not cancel writes")
	}
}

func TestTransactions_SchemaValidationInsideTransaction(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	cfg := globals.GetConfig()
	cfg.Api.Schema.Enabled = true
	globals.SetConfig(cfg)

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var out map[string]any
	_ = json.Unmarshal(begin.Body(), &out)
	txID := out["transaction_id"].(string)

	ok1 := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/users", map[string]any{
		"name": "Alice",
		"age":  25,
	})
	if ok1.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("valid write rejected")
	}

	bad := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/users", map[string]any{
		"name": 123,
	})
	if bad.StatusCode() == fasthttp.StatusOK {
		t.Fatalf("invalid schema accepted")
	}
}

func TestTransactions_SubEntitiesInsideTransaction(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var o map[string]any
	_ = json.Unmarshal(begin.Body(), &o)
	txID := o["transaction_id"].(string)

	resp := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/articles", map[string]any{
		"title": "Nested",
		"author": map[string]any{
			"@entity":  "author",
			"fullname": "Taymour",
		},
	})
	if resp.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("unexpected status %d", resp.StatusCode())
	}

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("commit failed")
	}

	list := mustGET(t, client, "http://test/api/author")
	var authors []map[string]any
	_ = json.Unmarshal(list.Body(), &authors)
	if len(authors) != 1 {
		t.Fatalf("expected 1 author, got %v", authors)
	}
}

func TestTransactions_VisibilityAfterCommit(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var o map[string]any
	_ = json.Unmarshal(begin.Body(), &o)
	txID := o["transaction_id"].(string)

	mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/posts", map[string]any{
		"title": "Hello",
	})

	before := mustGET(t, client, "http://test/api/posts")
	var b []map[string]any
	_ = json.Unmarshal(before.Body(), &b)
	if len(b) != 0 {
		t.Fatalf("visible before commit")
	}

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("commit failed")
	}

	after := mustGET(t, client, "http://test/api/posts")
	var a []map[string]any
	_ = json.Unmarshal(after.Body(), &a)
	if len(a) != 1 {
		t.Fatalf("expected 1 after commit")
	}
}

func TestTransactions_MultipleOperationsInOrder(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	begin := mustPOSTJSON(t, client, "http://test/api/tx/begin", nil)
	var o map[string]any
	_ = json.Unmarshal(begin.Body(), &o)
	txID := o["transaction_id"].(string)

	mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/x", map[string]any{
		"v": 1,
	})
	mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/entity/x", map[string]any{
		"v": 2,
	})

	commit := mustPOSTJSON(t, client, "http://test/api/tx/"+txID+"/commit", nil)
	if commit.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("commit failed")
	}

	all := mustGET(t, client, "http://test/api/x")
	var arr []map[string]any
	_ = json.Unmarshal(all.Body(), &arr)
	if len(arr) != 2 {
		t.Fatalf("expected 2, got %v", arr)
	}
}
