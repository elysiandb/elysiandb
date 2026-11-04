package memory

import (
	"runtime/metrics"
	"sync"
	"sync/atomic"
	"time"
)

type Level int32

const (
	Normal Level = iota
	High
	Critical
)

const (
	highShare       = 0.70
	criticalShare   = 0.85
	overshootShare  = 1.05
	defaultInterval = 50 * time.Millisecond
)

var (
	budget atomic.Int64
	level  atomic.Int32
	live   atomic.Int64
	used   atomic.Int64

	samplerMu sync.Mutex
	stopCh    chan struct{}
	samples   = []metrics.Sample{
		{Name: "/gc/heap/live:bytes"},
		{Name: "/memory/classes/total:bytes"},
		{Name: "/memory/classes/heap/released:bytes"},
	}
)

func SetBudget(bytes int64) {
	budget.Store(bytes)
	Sample()
}

func Budget() int64 {
	return budget.Load()
}

func Current() Level {
	return Level(level.Load())
}

func UnderPressure() bool {
	return Current() >= High
}

func IsCritical() bool {
	return Current() == Critical
}

type Stats struct {
	Budget int64
	Live   int64
	Used   int64
	Level  Level
}

func Snapshot() Stats {
	return Stats{Budget: budget.Load(), Live: live.Load(), Used: used.Load(), Level: Current()}
}

func Sample() {
	samplerMu.Lock()
	metrics.Read(samples)
	l := int64(samples[0].Value.Uint64())
	u := int64(samples[1].Value.Uint64()) - int64(samples[2].Value.Uint64())
	samplerMu.Unlock()

	live.Store(l)
	used.Store(u)
	level.Store(int32(levelFor(budget.Load(), l, u)))
}

func levelFor(b, l, u int64) Level {
	if b <= 0 {
		return Normal
	}

	switch {
	case float64(l) >= criticalShare*float64(b) || float64(u) >= overshootShare*float64(b):
		return Critical
	case float64(l) >= highShare*float64(b):
		return High
	}

	return Normal
}

func Start() {
	samplerMu.Lock()
	defer samplerMu.Unlock()

	if stopCh != nil {
		return
	}

	stop := make(chan struct{})
	stopCh = stop
	go func() {
		ticker := time.NewTicker(defaultInterval)
		defer ticker.Stop()
		for {
			select {
			case <-stop:
				return
			case <-ticker.C:
				Sample()
			}
		}
	}()
}
