package transaction_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/transaction"
)

type fakeStorage struct {
	writeErr    bool
	updateFail  bool
	deleteHit   bool
	writeCount  int
	updateCount int
}

func (f *fakeStorage) WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError {
	if f.writeErr {
		return []schema.ValidationError{{Message: "x"}}
	}
	f.writeCount++
	return nil
}

func (f *fakeStorage) UpdateEntityById(entity, id string, data map[string]interface{}) map[string]interface{} {
	if f.updateFail {
		return nil
	}
	f.updateCount++
	return map[string]interface{}{"ok": true}
}

func (f *fakeStorage) DeleteEntityById(entity, id string) {
	f.deleteHit = true
}

func TestBeginTransaction(t *testing.T) {
	tx := transaction.BeginTransaction()
	if tx == nil {
		t.Fatalf("nil tx")
	}
	if tx.ID == "" {
		t.Fatalf("empty id")
	}
}

func TestGetTransaction_OK(t *testing.T) {
	tx := transaction.BeginTransaction()
	got, err := transaction.GetTransaction(tx.ID)
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}
	if got.ID != tx.ID {
		t.Fatalf("wrong tx")
	}
}

func TestGetTransaction_NotFound(t *testing.T) {
	_, err := transaction.GetTransaction("missing")
	if err == nil {
		t.Fatalf("expected error")
	}
}

func TestAddOperation_OK(t *testing.T) {
	tx := transaction.BeginTransaction()
	op := transaction.TxOperation{
		Kind:   "write",
		Entity: "x",
		Data:   map[string]interface{}{"a": 1},
	}
	err := transaction.AddOperation(tx.ID, op)
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}
	got, _ := transaction.GetTransaction(tx.ID)
	if len(got.Ops) != 1 {
		t.Fatalf("op not added")
	}
}

func TestAddOperation_NotFound(t *testing.T) {
	err := transaction.AddOperation("missing", transaction.TxOperation{})
	if err == nil {
		t.Fatalf("expected error")
	}
}

func TestRollbackTransaction_OK(t *testing.T) {
	tx := transaction.BeginTransaction()
	err := transaction.RollbackTransaction(tx.ID)
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}
	_, err = transaction.GetTransaction(tx.ID)
	if err == nil {
		t.Fatalf("should be removed")
	}
}

func TestRollbackTransaction_NotFound(t *testing.T) {
	err := transaction.RollbackTransaction("missing")
	if err != nil {
		t.Fatalf("unexpected err")
	}
}

func TestCommitTransaction_WriteValidationFails(t *testing.T) {
	f := &fakeStorage{writeErr: true}
	orig := transaction.StorageImpl()
	transaction.SetStorageImpl(f)
	defer transaction.SetStorageImpl(orig)

	tx := transaction.BeginTransaction()
	transaction.AddOperation(tx.ID, transaction.TxOperation{
		Kind:   "write",
		Entity: "x",
		Data:   map[string]interface{}{"a": 1},
	})

	err := transaction.CommitTransaction(tx.ID)
	if err == nil {
		t.Fatalf("expected error")
	}
}

func TestCommitTransaction_UpdateFails(t *testing.T) {
	f := &fakeStorage{updateFail: true}
	orig := transaction.StorageImpl()
	transaction.SetStorageImpl(f)
	defer transaction.SetStorageImpl(orig)

	tx := transaction.BeginTransaction()
	transaction.AddOperation(tx.ID, transaction.TxOperation{
		Kind:   "update",
		Entity: "x",
		ID:     "1",
		Data:   map[string]interface{}{"a": 1},
	})

	err := transaction.CommitTransaction(tx.ID)
	if err == nil {
		t.Fatalf("expected error")
	}
}

func TestCommitTransaction_Delete_OK(t *testing.T) {
	f := &fakeStorage{}
	orig := transaction.StorageImpl()
	transaction.SetStorageImpl(f)
	defer transaction.SetStorageImpl(orig)

	tx := transaction.BeginTransaction()
	transaction.AddOperation(tx.ID, transaction.TxOperation{
		Kind:   "delete",
		Entity: "x",
		ID:     "1",
	})

	err := transaction.CommitTransaction(tx.ID)
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}
	if !f.deleteHit {
		t.Fatalf("delete not called")
	}
}

func TestCommitTransaction_NotFound(t *testing.T) {
	err := transaction.CommitTransaction("missing")
	if err == nil {
		t.Fatalf("expected error")
	}
}

func TestCommitTransaction_WriteAndUpdate_Success(t *testing.T) {
	f := &fakeStorage{}
	orig := transaction.StorageImpl()
	transaction.SetStorageImpl(f)
	defer transaction.SetStorageImpl(orig)

	tx := transaction.BeginTransaction()
	transaction.AddOperation(tx.ID, transaction.TxOperation{
		Kind:   "write",
		Entity: "a",
		Data:   map[string]interface{}{"x": 1},
	})
	transaction.AddOperation(tx.ID, transaction.TxOperation{
		Kind:   "update",
		Entity: "a",
		ID:     "1",
		Data:   map[string]interface{}{"y": 2},
	})

	err := transaction.CommitTransaction(tx.ID)
	if err != nil {
		t.Fatalf("unexpected err %v", err)
	}

	if f.writeCount != 1 || f.updateCount != 1 {
		t.Fatalf("ops not called correctly")
	}
}

func TestGenerateTxID(t *testing.T) {
	id := time.Now().Format("20060102150405.000000000")
	_ = id
	got := transaction.BeginTransaction()
	if got.ID == "" {
		t.Fatalf("missing id")
	}
}
