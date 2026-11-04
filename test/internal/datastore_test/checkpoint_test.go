package datastore_test

import (
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
)

func TestStatsAnswerDuringACheckpoint(t *testing.T) {
	opts := options(t.TempDir())
	opts.SegmentSize = 0
	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	putDocs(t, e, "big", 0, 60000, workDoc)
	for _, f := range []string{"a", "b", "c", "text"} {
		e.EnsureIndex("big", datastore.IndexSpec{Kind: datastore.IndexFilter, Field: f})
	}

	var slowest, longestCheckpoint time.Duration
	for round := 0; round < 3; round++ {
		putDocs(t, e, "big", 60000+round, 60001+round, workDoc)

		done := make(chan time.Duration, 1)
		go func() {
			t0 := time.Now()
			if err := e.Checkpoint(); err != nil {
				t.Error(err)
			}
			done <- time.Since(t0)
		}()

		for waiting := true; waiting; {
			select {
			case d := <-done:
				longestCheckpoint = max(longestCheckpoint, d)
				waiting = false
			default:
				t0 := time.Now()
				_ = e.Stats()
				slowest = max(slowest, time.Since(t0))
				time.Sleep(time.Millisecond)
			}
		}
	}

	if longestCheckpoint < 20*time.Millisecond {
		t.Skipf("checkpoints too fast to observe (%s)", longestCheckpoint)
	}
	if slowest > longestCheckpoint/4 {
		t.Fatalf("Stats waited %s during a checkpoint of %s", slowest, longestCheckpoint)
	}
}
