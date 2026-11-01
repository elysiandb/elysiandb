package recovery

import (
	"encoding/json"
	"os"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
)

const RecoveryFile = "elysiandb.json.recovery.log"

var (
	recoveryMu        sync.Mutex
	recoveryLogActive = false
)

var SaveJsonDBFunc func()

type recoveryOp struct {
	Op    string                 `json:"op"`
	Key   string                 `json:"key"`
	Value map[string]interface{} `json:"value,omitempty"`
}

func ActivateJsonRecoveryLog(saveDBFunc func()) {
	recoveryLogActive = true
	SaveJsonDBFunc = saveDBFunc
}

func appendJsonRecoveryOp(op recoveryOp) {
	recoveryMu.Lock()
	defer recoveryMu.Unlock()

	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + RecoveryFile
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

	checkJsonLogSize(path)
}

func LogJsonPut(key string, value map[string]any) {
	if !recoveryLogActive {
		return
	}

	appendJsonRecoveryOp(recoveryOp{Op: "put", Key: key, Value: value})
}

func LogJsonDelete(key string) {
	if !recoveryLogActive {
		return
	}
	appendJsonRecoveryOp(recoveryOp{Op: "del", Key: key})
}

func ReplayJsonRecoveryLog(
	putFunc func(key string, value map[string]any) error,
	deleteFunc func(key string),
) int {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + RecoveryFile
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return 0
		}

		recoveryLog.Error("cannot open the recovery log for replay", "log", "json", "file", path, "err", err)

		return 0
	}

	defer f.Close()

	start := time.Now()
	entries, unreadable := 0, 0
	err = replayLines(f, func(line []byte) {
		var op recoveryOp
		if err := json.Unmarshal(line, &op); err != nil {
			unreadable++
			return
		}

		switch op.Op {
		case "put":
			_ = putFunc(op.Key, op.Value)
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

	reportReplay("json", path, entries, unreadable, start)
	ClearJsonRecoveryLog()

	return entries
}

func ClearJsonRecoveryLog() {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + RecoveryFile
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		recoveryLog.Error("cannot clear the recovery log", "log", "json", "file", path, "err", err)
	}
}

func checkJsonLogSize(path string) {
	info, err := os.Stat(path)
	if err != nil {
		return
	}

	if info.Size() >= globals.GetConfig().Store.CrashRecovery.MaxLogMB*1024*1024 {
		reportForcedFlush("json", info.Size())
		SaveJsonDBFunc()
	}
}
