package log

import (
	"fmt"
	"io"
	"log/slog"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	"golang.org/x/term"
)

const (
	FormatText = "text"
	FormatJSON = "json"
)

type Options struct {
	Level       string
	Format      string
	BufferLines int
}

type stdoutWriter struct{}

func (stdoutWriter) Write(p []byte) (int, error) {
	return os.Stdout.Write(p)
}

type state struct {
	mu     sync.Mutex
	out    io.Writer
	format string
	color  bool
}

var (
	current         = &state{out: stdoutWriter{}, format: FormatText}
	levelVar        = new(slog.LevelVar)
	configuredLevel atomic.Int64
	root            = slog.New(&handler{})
	warningCount    atomic.Uint64
	errorCount      atomic.Uint64
	exit            = os.Exit
)

func Configure(opts Options) error {
	lvl, err := ParseLevel(opts.Level)
	if err != nil {
		return err
	}

	format := strings.ToLower(strings.TrimSpace(opts.Format))
	switch format {
	case "", FormatText:
		format = FormatText
	case FormatJSON:
	default:
		return fmt.Errorf("unknown log format %q: use text or json", opts.Format)
	}

	current.mu.Lock()
	current.format = format
	current.color = format == FormatText && StdoutIsTerminal() && os.Getenv("NO_COLOR") == ""
	current.mu.Unlock()

	configuredLevel.Store(int64(lvl))
	cancelOverride()
	levelVar.Set(lvl)
	resizeBuffer(opts.BufferLines)

	return nil
}

func SetOutput(w io.Writer, color bool) {
	current.mu.Lock()
	defer current.mu.Unlock()

	if w == nil {
		w = stdoutWriter{}
	}

	current.out = w
	current.color = color
}

func Format() string {
	current.mu.Lock()
	defer current.mu.Unlock()

	return current.format
}

func StdoutIsTerminal() bool {
	return term.IsTerminal(int(os.Stdout.Fd()))
}

func ParseLevel(value string) (slog.Level, error) {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "", "info":
		return slog.LevelInfo, nil
	case "debug":
		return slog.LevelDebug, nil
	case "warn", "warning":
		return slog.LevelWarn, nil
	case "error":
		return slog.LevelError, nil
	}

	return slog.LevelInfo, fmt.Errorf("unknown log level %q: use debug, info, warn or error", value)
}

func LevelName(level slog.Level) string {
	switch {
	case level < slog.LevelInfo:
		return "debug"
	case level < slog.LevelWarn:
		return "info"
	case level < slog.LevelError:
		return "warn"
	}

	return "error"
}

func Enabled(level slog.Level) bool {
	return level >= levelVar.Level()
}

func For(component string) *slog.Logger {
	return root.With("component", component)
}

func Debug(msg string, args ...any) {
	root.Debug(msg, args...)
}

func Info(msg string, args ...any) {
	root.Info(msg, args...)
}

func Warn(msg string, args ...any) {
	root.Warn(msg, args...)
}

func Error(msg string, args ...any) {
	root.Error(msg, args...)
}

func Fatal(msg string, args ...any) {
	root.Error(msg, args...)
	exit(1)
}

func Counts() (uint64, uint64) {
	return warningCount.Load(), errorCount.Load()
}

func Since(start time.Time) float64 {
	return float64(time.Since(start).Microseconds()) / 1000
}
