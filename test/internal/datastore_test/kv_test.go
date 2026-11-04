package datastore_test

import (
	"sort"
	"sync/atomic"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/datastore"
	"github.com/taymour/elysiandb/internal/docpath"
)

func TestKVMatchFollowsTheGlobAndTheLimit(t *testing.T) {
	e, err := datastore.Open(options(t.TempDir()))
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	keys := []string{"User:1", "user:2", "USER:3", "user:10", "users", "other", "us", "user:\x00x", "émile:1", "Émile:2"}
	for _, k := range keys {
		if err := e.KVPut(k, []byte("v-"+k), 0); err != nil {
			t.Fatal(err)
		}
	}

	all := e.KVKeys()
	if !sort.StringsAreSorted(all) || len(all) != len(keys) {
		t.Fatalf("every key is listed once and sorted, got %q", all)
	}

	for _, pattern := range []string{"user:*", "User:*", "us*", "*", "émile:*", "Émile*", "user:1", "*:2", "nothing*"} {
		want := []string{}
		for _, k := range all {
			if docpath.MatchGlob(pattern, k) {
				want = append(want, k)
			}
		}

		got := []string{}
		e.KVMatch(pattern, 0, func(key string) bool {
			got = append(got, key)
			return true
		})
		sort.Strings(got)
		if len(got) != len(want) {
			t.Fatalf("pattern %q: got %q, want %q", pattern, got, want)
		}
		for i := range got {
			if got[i] != want[i] {
				t.Fatalf("pattern %q: got %q, want %q", pattern, got, want)
			}
		}

		if len(want) > 1 {
			limited := 0
			e.KVMatch(pattern, 1, func(string) bool {
				limited++
				return true
			})
			if limited != 1 {
				t.Fatalf("pattern %q with a limit of 1 gave %d keys", pattern, limited)
			}

			stopped := 0
			e.KVMatch(pattern, 0, func(string) bool {
				stopped++
				return false
			})
			if stopped != 1 {
				t.Fatalf("pattern %q stops when the callback says so, got %d", pattern, stopped)
			}
		}
	}

	if v, _, ok := e.KVGet("user:\x00x"); !ok || string(v) != "v-user:\x00x" {
		t.Fatalf("a key holding a zero byte is read back, got %q %v", v, ok)
	}
}

func TestKVExpirationsAndCounts(t *testing.T) {
	opts := options(t.TempDir())
	var expired atomic.Int64
	opts.OnKVExpired = func(n int) { expired.Add(int64(n)) }
	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}

	now := time.Now().Unix()
	check := func(step string, keys, withTTL int64) {
		t.Helper()
		k, w := e.KVCount()
		if k != keys || w != withTTL {
			t.Fatalf("%s: %d keys and %d with a TTL, want %d and %d", step, k, w, keys, withTTL)
		}
	}

	_ = e.KVPut("plain", []byte("1"), 0)
	_ = e.KVPut("soon", []byte("2"), now+10)
	_ = e.KVPut("later", []byte("3"), now+1000)
	_ = e.KVPut("moved", []byte("4"), now+10)
	check("after the puts", 4, 3)

	_ = e.KVPut("moved", []byte("5"), now+2000)
	_ = e.KVPut("later", []byte("6"), 0)
	check("after changing the TTLs", 4, 2)

	if exp, ok := e.KVExpiration("moved"); !ok || exp != now+2000 {
		t.Fatalf("the new expiration replaces the old one, got %d %v", exp, ok)
	}
	if _, ok := e.KVExpiration("later"); ok {
		t.Fatalf("a key put without TTL has no expiration")
	}

	if n := e.ExpireDue(now + 20); n != 1 {
		t.Fatalf("only the due key expires, %d expired", n)
	}
	if _, _, ok := e.KVGet("soon"); ok {
		t.Fatalf("the due key is gone")
	}
	if _, _, ok := e.KVGet("moved"); !ok {
		t.Fatalf("a key whose expiration moved later stays")
	}
	check("after the expiration", 3, 1)
	if expired.Load() != 1 {
		t.Fatalf("the expirations are reported, got %d", expired.Load())
	}

	if !e.KVDelete("moved") || e.KVDelete("moved") || e.KVDelete("missing") {
		t.Fatalf("a delete reports whether the key existed")
	}
	if n := e.ExpireDue(now + 3000); n != 0 {
		t.Fatalf("a deleted key does not expire again, %d expired", n)
	}
	check("after the delete", 2, 0)

	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	e, err = datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	check("after a restart", 2, 0)

	_ = e.KVPut("ttl", []byte("x"), now+500)
	if err := e.KVReset(); err != nil {
		t.Fatal(err)
	}
	check("after a reset", 0, 0)
	if len(e.KVKeys()) != 0 {
		t.Fatalf("a reset removes every key")
	}
	if n := e.ExpireDue(now + 1000); n != 0 {
		t.Fatalf("a reset removes the expirations, %d expired", n)
	}

	if err := e.Close(); err != nil {
		t.Fatal(err)
	}
	if err := e.KVPut("closed", []byte("x"), 0); err == nil {
		t.Fatalf("a closed engine refuses writes")
	}
	if e.KVDelete("plain") || e.KVReset() == nil {
		t.Fatalf("a closed engine refuses deletes and resets")
	}
}

func TestExpiredKeysLeaveInTheBackground(t *testing.T) {
	opts := options(t.TempDir())
	opts.Background = true
	var expired atomic.Int64
	opts.OnKVExpired = func(n int) { expired.Add(int64(n)) }
	e, err := datastore.Open(opts)
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close()

	if err := e.KVPut("flash", []byte("x"), time.Now().Unix()+1); err != nil {
		t.Fatal(err)
	}

	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, _, ok := e.KVGet("flash"); !ok {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("an expired key is removed by the background loop")
		}
		time.Sleep(50 * time.Millisecond)
	}

	if expired.Load() != 1 {
		t.Fatalf("the background expiration is reported, got %d", expired.Load())
	}
}
