package recovery

import (
	"bufio"
	"encoding/json"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
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
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		log.Error("Error opening store recovery log:", err)
		return
	}
	defer f.Close()

	enc := json.NewEncoder(f)
	if err := enc.Encode(op); err != nil {
		log.Error("Error writing to store recovery log:", err)
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
	putFunc func(key string, value []byte, ttl int) error,
	deleteFunc func(key string),
) {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + StoreRecoveryFile
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return
		}
		log.Error("Error opening store recovery log for replay:", err)
		return
	}
	defer f.Close()

	now := int(time.Now().Unix())
	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var op storeRecoveryOp
		if err := json.Unmarshal([]byte(line), &op); err != nil {
			log.Error("Error decoding store recovery log entry:", err)
			continue
		}
		switch op.Op {
		case "put":
			if op.TTL > 0 && now >= op.TTL {
				continue
			}
			_ = putFunc(op.Key, op.Value, op.TTL)
		case "del":
			deleteFunc(op.Key)
		}
	}

	if err := scanner.Err(); err != nil {
		log.Error("Error scanning store recovery log:", err)
	}

	log.Info("Recovery MainStore log replay completed.")
	ClearStoreRecoveryLog()
}

func ClearStoreRecoveryLog() {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + StoreRecoveryFile
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		log.Error("Error clearing store recovery log:", err)
	}
}

func checkStoreLogSize(path string) {
	info, err := os.Stat(path)
	if err != nil {
		return
	}
	if info.Size() >= globals.GetConfig().Store.CrashRecovery.MaxLogMB*1024*1024 {
		SaveDBFunc()
	}
}
