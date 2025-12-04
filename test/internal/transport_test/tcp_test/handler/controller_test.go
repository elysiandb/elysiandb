package handler_test

import (
	"bytes"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/tcp/handler"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Stats.Enabled = false
	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.ResetStore()
}

func TestHandleSetAndGet(t *testing.T) {
	setup(t)

	if !bytes.Equal(handler.HandleSet([]byte("a 1"), 0), []byte("OK")) {
		t.Fatal("set failed")
	}

	out := handler.HandleGet([]byte("a"))
	if !bytes.Equal(out, []byte("a=1")) {
		t.Fatal("get failed")
	}
}

func TestHandleSetWithTTL(t *testing.T) {
	setup(t)

	if !bytes.Equal(handler.HandleSet([]byte("ttl 123"), 1), []byte("OK")) {
		t.Fatal("set ttl failed")
	}

	out := handler.HandleGet([]byte("ttl"))
	if !bytes.Equal(out, []byte("ttl=123")) {
		t.Fatal("get ttl failed")
	}
}

func TestHandleGetNotFound(t *testing.T) {
	setup(t)

	out := handler.HandleGet([]byte("missing"))
	if !bytes.Equal(out, []byte("missing=not found")) {
		t.Fatal("should not exist")
	}
}

func TestHandleDeleteSingle(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("x 9"), 0)

	out := handler.HandleDelete([]byte("x"))
	if !bytes.Equal(out, []byte("Deleted 1")) {
		t.Fatal("delete failed")
	}

	out = handler.HandleGet([]byte("x"))
	if !bytes.Equal(out, []byte("x=not found")) {
		t.Fatal("delete ineffective")
	}
}

func TestHandleReset(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("a 1"), 0)
	handler.HandleReset()

	out := handler.HandleGet([]byte("a"))
	if !bytes.Equal(out, []byte("a=not found")) {
		t.Fatal("reset failed")
	}
}

func TestHandleSave(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("a 1"), 0)

	out := handler.HandleSave()
	if !bytes.Equal(out, []byte("OK")) {
		t.Fatal("save failed")
	}
}

func TestHandleMultiGet(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("a 1"), 0)
	handler.HandleSet([]byte("b 2"), 0)

	out := handler.HandleMultiGet([]byte("a b c"))
	if !bytes.Contains(out, []byte("1")) || !bytes.Contains(out, []byte("2")) || !bytes.Contains(out, []byte("c=not found")) {
		t.Fatal("mget failed")
	}
}

func TestHandleWildcardGet(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("user:1 a"), 0)
	handler.HandleSet([]byte("user:2 b"), 0)

	out := handler.HandleGet([]byte("user:*"))
	if !bytes.Contains(out, []byte("user:1")) || !bytes.Contains(out, []byte("user:2")) {
		t.Fatal("wildcard get failed")
	}
}

func TestHandleWildcardDelete(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("wild:1 a"), 0)
	handler.HandleSet([]byte("wild:2 b"), 0)

	out := handler.HandleDelete([]byte("wild:*"))
	if !bytes.HasPrefix(out, []byte("Deleted ")) {
		t.Fatal("wildcard delete wrong prefix")
	}

	out2 := handler.HandleGet([]byte("wild:*"))
	if !bytes.Contains(out2, []byte("not found")) {
		t.Fatal("wildcard delete ineffective")
	}
}

func TestHandleMultiGetWildcard(t *testing.T) {
	setup(t)

	handler.HandleSet([]byte("a:1 x"), 0)
	handler.HandleSet([]byte("a:2 y"), 0)

	out := handler.HandleMultiGet([]byte("a:*"))
	if !bytes.Contains(out, []byte("a:1")) || !bytes.Contains(out, []byte("a:2")) {
		t.Fatal("mget wildcard failed")
	}
}
