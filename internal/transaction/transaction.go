package transaction

import (
	"errors"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/schema"
)

type TxOperation struct {
	Kind   string
	Entity string
	ID     string
	Data   map[string]interface{}
}

type Transaction struct {
	ID        string
	Ops       []TxOperation
	StartedAt time.Time
}

func StorageImpl() Storage {
	return storageImpl
}

func SetStorageImpl(s Storage) {
	storageImpl = s
}

type Storage interface {
	WriteEntity(entity string, data map[string]interface{}) []schema.ValidationError
	UpdateEntityById(entity, id string, data map[string]interface{}) map[string]interface{}
	DeleteEntityById(entity, id string)
}

type realStorage struct{}

func (realStorage) WriteEntity(e string, d map[string]interface{}) []schema.ValidationError {
	return engine.WriteEntity(e, d)
}

func (realStorage) UpdateEntityById(e, id string, d map[string]interface{}) map[string]interface{} {
	return engine.UpdateEntityById(e, id, d)
}

func (realStorage) DeleteEntityById(e, id string) {
	engine.DeleteEntityById(e, id)
}

var storageImpl Storage = realStorage{}

var TxManager = struct {
	mu  sync.Mutex
	txs map[string]*Transaction
}{txs: map[string]*Transaction{}}

func BeginTransaction() *Transaction {
	TxManager.mu.Lock()
	defer TxManager.mu.Unlock()

	txID := generateTxID()
	tx := &Transaction{
		ID:        txID,
		Ops:       []TxOperation{},
		StartedAt: time.Now(),
	}
	TxManager.txs[txID] = tx

	return tx
}

func generateTxID() string {
	return time.Now().Format("20060102150405.000000000")
}

func GetTransaction(txID string) (*Transaction, error) {
	TxManager.mu.Lock()
	defer TxManager.mu.Unlock()

	tx, ok := TxManager.txs[txID]
	if !ok {
		return nil, errors.New("transaction not found")
	}

	return tx, nil
}

func AddOperation(txID string, op TxOperation) error {
	TxManager.mu.Lock()
	defer TxManager.mu.Unlock()

	tx, ok := TxManager.txs[txID]
	if !ok {
		return errors.New("transaction not found")
	}

	tx.Ops = append(tx.Ops, op)

	return nil
}

func CommitTransaction(txID string) error {
	TxManager.mu.Lock()
	tx, ok := TxManager.txs[txID]
	if !ok {
		TxManager.mu.Unlock()

		return errors.New("transaction not found")
	}

	delete(TxManager.txs, txID)
	TxManager.mu.Unlock()

	for _, op := range tx.Ops {
		switch op.Kind {
		case "write":
			errs := storageImpl.WriteEntity(op.Entity, op.Data)
			if len(errs) > 0 {
				return errors.New("validation error")
			}
		case "update":
			res := storageImpl.UpdateEntityById(op.Entity, op.ID, op.Data)
			if res == nil {
				return errors.New("update failed")
			}
		case "delete":
			storageImpl.DeleteEntityById(op.Entity, op.ID)
		}
	}

	return nil
}

func RollbackTransaction(txID string) error {
	TxManager.mu.Lock()
	defer TxManager.mu.Unlock()

	if _, exists := TxManager.txs[txID]; !exists {
		return nil
	}

	delete(TxManager.txs, txID)

	return nil
}
