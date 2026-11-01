package recovery

import (
	"encoding/json"
	"os"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
)

const StoreRecoveryFile = "elysiandb.store.recovery.log"

var (
	storeRecoveryMu     sync.Mutex
	storeRecoveryActive = false
)

var SaveDBFunc func()

type storeRecoveryOp struct {
	Op    string `json:"op"`
	Key   string `json:"key"`
	Value []byte `json:"value,omitempty"`
	TTL   int    `json:"ttl,omitempty"`
}

func ActivateStoreRecoveryLog(saveDBFunc func()) {
	storeRecoveryActive = true
	SaveDBFunc = saveDBFunc
}

func appendStoreRecoveryOp(op storeRecoveryOp) {
	storeRecoveryMu.Lock()
	defer storeRecoveryMu.Unlock()

	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + StoreRecoveryFile
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		reportWriteFailure(path, err)
		return
	}

	_ = f.Chmod(0o600)

	defer f.Close()

	enc := json.NewEncoder(f)
	if err := enc.Encode(op); err != nil {
		reportWriteFailure(path, err)
	}

	checkStoreLogSize(path)
}

func LogStorePut(key string, value []byte) {
	if !storeRecoveryActive {
		return
	}

	appendStoreRecoveryOp(storeRecoveryOp{Op: "put", Key: key, Value: value})
}

func LogStorePutWithTTL(key string, value []byte, ttl int) {
	if !storeRecoveryActive {
		return
	}

	var exp int
	if ttl > 0 {
		exp = int(time.Now().Unix()) + ttl
	}

	appendStoreRecoveryOp(storeRecoveryOp{Op: "put", Key: key, Value: value, TTL: exp})
}

func LogStoreDelete(key string) {
	if !storeRecoveryActive {
		return
	}

	appendStoreRecoveryOp(storeRecoveryOp{Op: "del", Key: key})
}

func ReplayStoreRecoveryLog(
	putFunc func(key string, value []byte, expiresAt int64) error,
	deleteFunc func(key string),
) int {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + StoreRecoveryFile
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return 0
		}

		recoveryLog.Error("cannot open the recovery log for replay", "log", "store", "file", path, "err", err)

		return 0
	}

	defer f.Close()

	start := time.Now()
	entries, unreadable := 0, 0

	now := int(time.Now().Unix())
	err = replayLines(f, func(line []byte) {
		var op storeRecoveryOp
		if err := json.Unmarshal(line, &op); err != nil {
			unreadable++
			return
		}

		switch op.Op {
		case "put":
			if op.TTL > 0 && now >= op.TTL {
				return
			}
			_ = putFunc(op.Key, op.Value, int64(op.TTL))
			entries++
		case "del":
			deleteFunc(op.Key)
			entries++
		}
	})
	if err != nil {
		keepUnreplayedLog(path, err)
		return entries
	}

	reportReplay("store", path, entries, unreadable, start)
	ClearStoreRecoveryLog()

	return entries
}

func ClearStoreRecoveryLog() {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + StoreRecoveryFile
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		recoveryLog.Error("cannot clear the recovery log", "log", "store", "file", path, "err", err)
	}
}

func checkStoreLogSize(path string) {
	info, err := os.Stat(path)
	if err != nil {
		return
	}

	if info.Size() >= globals.GetConfig().Store.CrashRecovery.MaxLogMB*1024*1024 {
		reportForcedFlush("store", info.Size())
		SaveDBFunc()
	}
}
