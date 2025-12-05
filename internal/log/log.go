package log

import (
	"fmt"
	"os"
	"sync"
	"time"
)

const (
	reset   = "\033[0m"
	red     = "\033[31m"
	green   = "\033[32m"
	yellow  = "\033[33m"
	magenta = "\033[35m"
	cyan    = "\033[36m"
)

type logsContainer struct {
	mu       sync.Mutex
	LogsList []string
}

var Logs = logsContainer{}

func Info(args ...interface{}) {
	go log("INFO", cyan, args...)
}

func DirectInfo(args ...interface{}) {
	directLog("INFO", cyan, args...)
}

func DirectError(args ...interface{}) {
	directLog("ERROR", red, args...)
}

func Success(args ...interface{}) {
	go log("SUCCESS", green, args...)
}

func Warn(args ...interface{}) {
	go log("WARN", yellow, args...)
}

func Error(args ...interface{}) {
	go log("ERROR", red, args...)
}

func Debug(args ...interface{}) {
	go log("DEBUG", magenta, args...)
}

func Fatal(message string, err error) {
	fmt.Printf("FATAL: %s: %v\n", message, err)
	os.Exit(1)
}

func log(level string, color string, args ...interface{}) {
	now := time.Now().Format("2006-01-02 15:04:05")
	msg := fmt.Sprint(args...)

	levelBadge := fmt.Sprintf("%s%s%s", color, level, reset)

	Logs.mu.Lock()
	defer Logs.mu.Unlock()
	Logs.LogsList = append(Logs.LogsList, fmt.Sprintf("[%s] %s %s", now, levelBadge, msg))
}

func directLog(level string, color string, args ...interface{}) {
	now := time.Now().Format("2006-01-02 15:04:05")
	msg := fmt.Sprint(args...)

	levelBadge := fmt.Sprintf("%s%s%s", color, level, reset)

	fmt.Printf("[%s] %s %s\n", now, levelBadge, msg)
}

func WriteLogs() {
	Logs.mu.Lock()
	logsList := Logs.LogsList
	Logs.LogsList = Logs.LogsList[:0]
	Logs.mu.Unlock()

	for _, log := range logsList {
		fmt.Println(log)
	}
}
