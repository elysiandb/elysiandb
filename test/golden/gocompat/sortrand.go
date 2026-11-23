package main

import (
	"encoding/base64"
	"errors"
	"math/rand"
	"sort"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

type keyed struct {
	key int
	idx int
}

func sortInputs() [][]int {
	r := rand.New(rand.NewSource(7))
	var out [][]int
	sizes := []int{0, 1, 2, 3, 4, 5, 6, 7, 8, 11, 12, 13, 16, 17, 24, 31, 32, 33, 49, 50, 64, 100, 128, 200, 500, 1000, 5000}
	for _, n := range sizes {
		for _, spread := range []int{1, 2, 3, 10, n + 1} {
			a := make([]int, n)
			for i := range a {
				a[i] = r.Intn(spread)
			}
			out = append(out, a)
		}
		sorted := make([]int, n)
		rev := make([]int, n)
		saw := make([]int, n)
		organ := make([]int, n)
		for i := 0; i < n; i++ {
			sorted[i] = i / 3
			rev[i] = (n - i) / 3
			saw[i] = i % 5
			if i < n/2 {
				organ[i] = i
			} else {
				organ[i] = n - i
			}
		}
		out = append(out, sorted, rev, saw, organ)
	}
	return out
}

func genSort(out string) error {
	var l goldenio.Lines
	for _, keys := range sortInputs() {
		a := make([]keyed, len(keys))
		b := make([]keyed, len(keys))
		for i, k := range keys {
			a[i] = keyed{k, i}
			b[i] = keyed{k, i}
		}
		sort.Slice(a, func(i, j int) bool { return a[i].key < a[j].key })
		sort.SliceStable(b, func(i, j int) bool { return b[i].key < b[j].key })
		perm := make([]int, len(a))
		stable := make([]int, len(b))
		for i := range a {
			perm[i] = a[i].idx
			stable[i] = b[i].idx
		}
		l.Add(map[string]any{"keys": keys, "slice": perm, "slice_stable": stable})
	}
	return l.Write(out, "sort/slice.jsonl")
}

var randSeeds = []int64{0, 1, 2, 3, 5, 7, 9, 11, 23, 42, 20261105, -1, -20261105, 1 << 62, 89482311}

func genRand(out string) error {
	var l goldenio.Lines
	seeds := append([]int64{}, randSeeds...)
	for w := int64(0); w < 8; w++ {
		seeds = append(seeds, w+10)
	}
	seen := map[int64]bool{}
	ns := []int{1, 2, 3, 7, 10, 26, 100, 1000, 1 << 20, 1<<31 - 1}
	n64 := []int64{1, 3, 1000, 1 << 31, 1<<31 + 1, 1 << 40, 1<<62 + 7, 1<<63 - 1}
	for _, seed := range seeds {
		if seen[seed] {
			continue
		}
		seen[seed] = true
		r := rand.New(rand.NewSource(seed))
		var int63 []string
		for i := 0; i < 700; i++ {
			int63 = append(int63, i64s(r.Int63()))
		}
		r = rand.New(rand.NewSource(seed))
		var ops []any
		for i := 0; i < 600; i++ {
			switch i % 6 {
			case 0, 1:
				n := ns[(i/6)%len(ns)]
				ops = append(ops, []any{"Intn", n, r.Intn(n)})
			case 2:
				ops = append(ops, []any{"Float64", fbits(r.Float64())})
			case 3:
				n := n64[(i/6)%len(n64)]
				ops = append(ops, []any{"Int63n", i64s(n), i64s(r.Int63n(n))})
			case 4:
				n := int32(ns[(i/6)%len(ns)])
				ops = append(ops, []any{"Int31n", n, r.Int31n(n)})
			case 5:
				if (i/6)%10 == 0 {
					ops = append(ops, []any{"Perm", 7, r.Perm(7)})
				} else {
					ops = append(ops, []any{"Uint64", u64s(r.Uint64())})
				}
			}
		}
		l.Add(map[string]any{"seed": i64s(seed), "int63": int63, "ops": ops})
	}
	return l.Write(out, "rand/sources.jsonl")
}

func genBase64(out string) error {
	var l goldenio.Lines
	r := rand.New(rand.NewSource(9))
	for n := 0; n <= 40; n++ {
		b := make([]byte, n)
		for i := range b {
			b[i] = byte(r.Intn(256))
		}
		l.Add(map[string]any{"fn": "encode", "in": hx(b), "std": base64.StdEncoding.EncodeToString(b), "raw_url": base64.RawURLEncoding.EncodeToString(b)})
	}
	inputs := []string{
		"", "QQ==", "QQ=", "QQ", "QUI=", "QUJD", "QUJDRA==", "QUJD\nRA==", "QUJD\r\nRA==", "QU JD", "QUJD====", "=QUJD", "QUJD!", "QUJ-", "QUJ_", "Q", "QUJDR",
		"aGVsbG8=", "aGVsbG8", "AA==", "AAA=", "AP8Q", "////", "++++", "QR==", "QUI==", "\n", "QUJD\n", "\xff", "QUJDRA=\n=", "QQ==QQ==",
	}
	for _, s := range inputs {
		row := map[string]any{"fn": "decode", "in": hs(s)}
		b, err := base64.StdEncoding.DecodeString(s)
		var ce base64.CorruptInputError
		switch {
		case err == nil:
			row["out"] = hx(b)
		case errors.As(err, &ce):
			row["corrupt_at"] = int64(ce)
			row["partial"] = hx(b)
		default:
			row["error"] = true
		}
		l.Add(row)
	}
	return l.Write(out, "base64/cases.jsonl")
}
