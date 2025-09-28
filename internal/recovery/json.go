package recovery

import (
	"bufio"
	"encoding/json"
	"os"
	"strings"
	"sync"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
)

const RecoveryFile = "elysiandb.json.recovery.log"

var recoveryMu sync.Mutex
var recoveryLogActive = false

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
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o644)
	if err != nil {
		log.Error("Error opening recovery log:", err)
		return
	}
	defer f.Close()

	enc := json.NewEncoder(f)
	if err := enc.Encode(op); err != nil {
		log.Error("Error writing to recovery log:", err)
	}

	checkJsonLogSize(path)
}

func LogJsonPut(key string, value map[string]interface{}) {
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
	putFunc func(key string, value map[string]interface{}) error,
	deleteFunc func(key string),
) {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + RecoveryFile
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return
		}
		log.Error("Error opening recovery log for replay:", err)
		return
	}
	defer f.Close()

	scanner := bufio.NewScanner(f)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var op recoveryOp
		if err := json.Unmarshal([]byte(line), &op); err != nil {
			log.Error("Error decoding recovery log entry:", err)
			continue
		}
		switch op.Op {
		case "put":
			_ = putFunc(op.Key, op.Value)
		case "del":
			deleteFunc(op.Key)
		}
	}

	if err := scanner.Err(); err != nil {
		log.Error("Error scanning recovery log:", err)
	}

	log.Info("Recovery Json log replay completed.")
	ClearJsonRecoveryLog()
}

func ClearJsonRecoveryLog() {
	cfg := globals.GetConfig()
	path := cfg.Store.Folder + "/" + RecoveryFile
	if err := os.Remove(path); err != nil && !os.IsNotExist(err) {
		log.Error("Error clearing recovery log:", err)
	}
}

func checkJsonLogSize(path string) {
	info, err := os.Stat(path)
	if err != nil {
		return
	}
	if info.Size() >= globals.GetConfig().Store.CrashRecovery.MaxLogMB*1024*1024 {
		SaveJsonDBFunc()
	}
}
