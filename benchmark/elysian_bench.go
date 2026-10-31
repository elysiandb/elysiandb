package main

import (
	"bufio"
	"bytes"
	"context"
	"flag"
	"fmt"
	"io"
	"math"
	mrand "math/rand"
	"net"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"time"
)

type opType uint8

const (
	opGET opType = iota
	opSET
)

type result struct {
	latency  time.Duration
	bytesIn  int
	bytesOut int
	errKind  string
}

type rolling struct {
	mu  sync.Mutex
	buf []time.Duration
}

func (r *rolling) push(d time.Duration) {
	r.mu.Lock()
	r.buf = append(r.buf, d)
	if len(r.buf) > 20000 {
		r.buf = r.buf[len(r.buf)-10000:]
	}
	r.mu.Unlock()
}

func (r *rolling) snapshot() []time.Duration {
	r.mu.Lock()
	cp := make([]time.Duration, len(r.buf))
	copy(cp, r.buf)
	r.mu.Unlock()
	return cp
}

func readableBps(bytes int64, secs float64) string {
	if secs <= 0 {
		return "0 B/s"
	}
	bps := float64(bytes) / secs
	const k = 1024.0
	switch {
	case bps >= k*k*k:
		return fmt.Sprintf("%.2f GB/s", bps/(k*k*k))
	case bps >= k*k:
		return fmt.Sprintf("%.2f MB/s", bps/(k*k))
	case bps >= k:
		return fmt.Sprintf("%.2f KB/s", bps/k)
	default:
		return fmt.Sprintf("%.0f B/s", bps)
	}
}

func percentile(ds []time.Duration, p float64) time.Duration {
	if len(ds) == 0 {
		return 0
	}
	sort.Slice(ds, func(i, j int) bool { return ds[i] < ds[j] })
	idx := int(math.Ceil(p*float64(len(ds)))) - 1
	if idx < 0 {
		idx = 0
	}
	if idx >= len(ds) {
		idx = len(ds) - 1
	}
	return ds[idx]
}

func asciiBar(p float64, width int) string {
	if p < 0 {
		p = 0
	}
	if p > 1 {
		p = 1
	}
	fill := int(math.Round(p * float64(width)))
	if fill > width {
		fill = width
	}
	return strings.Repeat("█", fill) + strings.Repeat("░", width-fill)
}

func genValue(n int) []byte {
	if n <= 0 {
		return []byte("x")
	}
	b := make([]byte, n)
	for i := range b {
		b[i] = 'A' + byte(mrand.Intn(16))
	}
	return b
}

func pickKey(keys int) string {
	i := mrand.Intn(keys)
	return fmt.Sprintf("key_%d", i)
}

func main() {
	pair := flag.Bool("pair", false, "Chain SET then GET on the same key (1:1). Ignores -setratio")
	addr := flag.String("addr", "127.0.0.1:8088", "TCP address (host:port)")
	vus := flag.Int("vus", 4, "Number of concurrent connections (VUs)")
	duration := flag.Duration("duration", 20*time.Second, "Test duration (e.g., 20s, 1m)")
	rps := flag.Int("rps", 0, "Global req/s limit (0 = unlimited)")
	keys := flag.Int("keys", 20000, "Number of distinct keys")
	setRatio := flag.Float64("setratio", 0.10, "SET fraction (0..1)")
	payloadLen := flag.Int("payload", 16, "SET value size (bytes)")
	warmup := flag.Bool("warmup", true, "Pre-fill the DB with SETs")
	showLive := flag.Bool("live", true, "Show a live summary every second")
	flag.StringVar(&authCredentials, "auth", "", "Credentials sent with AUTH on each connection: '<token>' or '<username> <password>'")
	flag.Parse()

	mrand.Seed(time.Now().UnixNano())

	fmt.Println()
	fmt.Println("  █ ElysianDB TCP Bench")
	fmt.Println("  ─────────────────────")
	fmt.Printf("  addr      : %s\n", *addr)
	fmt.Printf("  vus       : %d\n", *vus)
	fmt.Printf("  duration  : %s\n", duration.String())
	if *rps > 0 {
		fmt.Printf("  rps limit : %d\n", *rps)
	} else {
		fmt.Printf("  rps limit : unlimited\n")
	}
	fmt.Printf("  keys      : %d\n", *keys)
	if *pair {
		fmt.Printf("  mode      : SET→GET paired (1:1)\n")
		fmt.Printf("  set ratio : (ignored in -pair)\n")
	} else {
		fmt.Printf("  set ratio : %.2f\n", *setRatio)
	}
	fmt.Printf("  payload   : %d bytes\n", *payloadLen)
	fmt.Printf("  warmup    : %v\n", *warmup)
	if *pair {
		fmt.Printf("  mode      : SET→GET paired (1:1)\n")
	}
	fmt.Println()

	value := genValue(*payloadLen)

	if *warmup && *keys > 0 {
		fmt.Println("  ▶ Warmup: SET keys…")
		start := time.Now()
		wp := int(math.Min(float64(*vus*2), 64))
		var wg sync.WaitGroup
		var setErrs int64
		ch := make(chan int, *keys)
		for i := 0; i < *keys; i++ {
			ch <- i
		}
		close(ch)
		for i := 0; i < wp; i++ {
			wg.Add(1)
			go func() {
				defer wg.Done()
				conn, err := dial(*addr)
				if err != nil {
					atomic.AddInt64(&setErrs, 1)
					return
				}
				defer conn.Close()
				r := bufio.NewReaderSize(conn, 128<<10)
				w := bufio.NewWriterSize(conn, 128<<10)
				for k := range ch {
					line := fmt.Sprintf("SET key_%d %s\n", k, value)
					if _, err := w.WriteString(line); err != nil {
						atomic.AddInt64(&setErrs, 1)
						return
					}
					if err := w.Flush(); err != nil {
						atomic.AddInt64(&setErrs, 1)
						return
					}
					if _, err := r.ReadSlice('\n'); err != nil {
						atomic.AddInt64(&setErrs, 1)
						return
					}
				}
			}()
		}
		wg.Wait()
		el := time.Since(start)
		if atomic.LoadInt64(&setErrs) > 0 {
			fmt.Printf("  ⚠ Warmup completed with %d errors in %s\n", setErrs, el)
		} else {
			fmt.Printf("  ✓ Warmup completed in %s\n", el)
		}
		fmt.Println()
	}

	var tick <-chan time.Time
	if *rps > 0 {
		tick = time.Tick(time.Second / time.Duration(*rps))
	}

	ctx, cancel := context.WithTimeout(context.Background(), *duration)
	defer cancel()

	results := make(chan result, 1024)

	var totalReq, totalErr, totalMiss, bytesIn, bytesOut int64

	var allLatencies []time.Duration
	var allMu sync.Mutex
	roll := &rolling{}

	var wg sync.WaitGroup
	for i := 0; i < *vus; i++ {
		wg.Add(1)
		go func(id int) {
			defer wg.Done()
			conn, err := dial(*addr)
			if err != nil {
				results <- result{errKind: "connect"}
				return
			}
			defer conn.Close()
			r := bufio.NewReaderSize(conn, 128<<10)
			w := bufio.NewWriterSize(conn, 128<<10)

			for {
				select {
				case <-ctx.Done():
					return
				default:
				}
				if tick != nil {
					<-tick
				}

				if *pair {
					// SET
					key := pickKey(*keys)
					{
						outLine := "SET " + key + " " + string(value) + "\n"
						t0 := time.Now()
						nw, werr := w.WriteString(outLine)
						if werr == nil {
							werr = w.Flush()
						}
						if werr != nil {
							results <- result{errKind: "write", bytesOut: nw}
							continue
						}
						in, rerr := r.ReadSlice('\n')
						lat := time.Since(t0)

						res := result{latency: lat, bytesOut: nw}
						if rerr != nil {
							if rerr != io.EOF {
								res.errKind = "read"
							} else {
								res.errKind = "eof"
							}
							results <- res
							return
						}
						res.bytesIn = len(in)
						if !bytes.HasPrefix(bytes.ToUpper(bytes.TrimSpace(in)), []byte("OK")) {
							res.errKind = "proto"
						}
						results <- res
					}
					// GET
					{
						outLine := "GET " + key + "\n"
						t0 := time.Now()
						nw, werr := w.WriteString(outLine)
						if werr == nil {
							werr = w.Flush()
						}
						if werr != nil {
							results <- result{errKind: "write", bytesOut: nw}
							continue
						}
						in, rerr := r.ReadSlice('\n')
						lat := time.Since(t0)

						res := result{latency: lat, bytesOut: nw}
						if rerr != nil {
							if rerr != io.EOF {
								res.errKind = "read"
							} else {
								res.errKind = "eof"
							}
							results <- res
							return
						}
						res.bytesIn = len(in)
						if bytes.HasPrefix(bytes.ToLower(bytes.TrimSpace(in)), []byte("key not found")) {
							res.errKind = "miss"
						}
						results <- res
					}
					continue
				}

				ot := opGET
				if mrand.Float64() < *setRatio {
					ot = opSET
				}
				key := pickKey(*keys)
				var outLine string
				if ot == opSET {
					outLine = "SET " + key + " " + string(value) + "\n"
				} else {
					outLine = "GET " + key + "\n"
				}

				t0 := time.Now()
				nw, werr := w.WriteString(outLine)
				if werr == nil {
					werr = w.Flush()
				}
				if werr != nil {
					results <- result{errKind: "write", bytesOut: nw}
					continue
				}
				in, rerr := r.ReadSlice('\n')
				lat := time.Since(t0)

				res := result{latency: lat, bytesOut: nw}
				if rerr != nil {
					if rerr != io.EOF {
						res.errKind = "read"
					} else {
						res.errKind = "eof"
					}
					results <- res
					return
				}
				res.bytesIn = len(in)

				if ot == opGET {
					if bytes.HasPrefix(bytes.ToLower(bytes.TrimSpace(in)), []byte("key not found")) {
						res.errKind = "miss"
					}
				} else {
					if !bytes.HasPrefix(bytes.ToUpper(bytes.TrimSpace(in)), []byte("OK")) {
						res.errKind = "proto"
					}
				}
				results <- res
			}
		}(i)
	}

	start := time.Now()
	var lastTotal, lastBytesIn, lastBytesOut int64

	liveTicker := time.NewTicker(1 * time.Second)
	defer liveTicker.Stop()

	go func() {
		wg.Wait()
		close(results)
	}()

	for {
		select {
		case r, ok := <-results:
			if !ok {
				goto END
			}
			atomic.AddInt64(&totalReq, 1)
			atomic.AddInt64(&bytesIn, int64(r.bytesIn))
			atomic.AddInt64(&bytesOut, int64(r.bytesOut))
			if r.errKind != "" && r.errKind != "miss" {
				atomic.AddInt64(&totalErr, 1)
			}
			if r.errKind == "miss" {
				atomic.AddInt64(&totalMiss, 1)
			}
			if r.latency > 0 {
				roll.push(r.latency)
				allMu.Lock()
				allLatencies = append(allLatencies, r.latency)
				allMu.Unlock()
			}
		case <-liveTicker.C:
			if *showLive {
				cur := atomic.LoadInt64(&totalReq)
				delta := cur - lastTotal
				lastTotal = cur

				bi := atomic.LoadInt64(&bytesIn)
				bo := atomic.LoadInt64(&bytesOut)
				dbi := bi - lastBytesIn
				dbo := bo - lastBytesOut
				lastBytesIn, lastBytesOut = bi, bo

				errs := atomic.LoadInt64(&totalErr)
				miss := atomic.LoadInt64(&totalMiss)
				ls := roll.snapshot()
				var p50, p95 time.Duration
				if len(ls) > 0 {
					p50 = percentile(ls, 0.50)
					p95 = percentile(ls, 0.95)
				}
				fmt.Printf("\r  %s  | VUs: %-3d | req/s: %-6d | p50: %-6s | p95: %-6s | errs: %-6d | miss: %-6d | in: %-10s | out: %-10s",
					time.Since(start).Truncate(time.Second).String(),
					*vus, delta, durStr(p50), durStr(p95), errs, miss,
					readableBps(dbi, 1), readableBps(dbo, 1),
				)
			}
		case <-ctx.Done():
			goto END
		}
	}

END:
	totalDur := time.Since(start)
	if *showLive {
		fmt.Println()
	}
	var lat []time.Duration
	allMu.Lock()
	if len(allLatencies) > 0 {
		lat = make([]time.Duration, len(allLatencies))
		copy(lat, allLatencies)
	}
	allMu.Unlock()

	var min, max, sum time.Duration
	if len(lat) > 0 {
		min, max = lat[0], lat[0]
		for _, d := range lat {
			if d < min {
				min = d
			}
			if d > max {
				max = d
			}
			sum += d
		}
	}
	avg := time.Duration(0)
	if len(lat) > 0 {
		avg = time.Duration(int64(sum) / int64(len(lat)))
	}
	p50 := percentile(lat, 0.50)
	p90 := percentile(lat, 0.90)
	p95 := percentile(lat, 0.95)
	p99 := percentile(lat, 0.99)

	reqs := atomic.LoadInt64(&totalReq)
	errs := atomic.LoadInt64(&totalErr)
	miss := atomic.LoadInt64(&totalMiss)
	bi := atomic.LoadInt64(&bytesIn)
	bo := atomic.LoadInt64(&bytesOut)

	thru := float64(reqs) / totalDur.Seconds()

	fmt.Println()
	fmt.Println("  ─────────────────────────────────────────────────────────────────")
	fmt.Println("  █ RESULTS")
	fmt.Println("  ─────────────────────────────────────────────────────────────────")
	fmt.Printf("  time           : %s\n", totalDur.Truncate(time.Millisecond))
	fmt.Printf("  vus            : %d\n", *vus)
	fmt.Printf("  requests       : %d\n", reqs)
	fmt.Printf("  successes      : %d\n", reqs-errs)
	fmt.Printf("  errors         : %d\n", errs)
	fmt.Printf("  misses (GET)   : %d\n", miss)
	fmt.Println()
	fmt.Printf("  throughput     : %.0f req/s\n", thru)
	if *pair {
		fmt.Printf("  throughput (pairs) : %.0f pairs/s\n", thru/2)
	}
	fmt.Printf("  transfer in    : %s\n", readableBps(bi, totalDur.Seconds()))
	fmt.Printf("  transfer out   : %s\n", readableBps(bo, totalDur.Seconds()))
	fmt.Println()
	fmt.Println("  Latency (ms)")
	fmt.Printf("    min          : %6.2f\n", float64(min.Microseconds())/1000.0)
	fmt.Printf("    p50          : %6.2f\n", float64(p50.Microseconds())/1000.0)
	fmt.Printf("    avg          : %6.2f\n", float64(avg.Microseconds())/1000.0)
	fmt.Printf("    p90          : %6.2f\n", float64(p90.Microseconds())/1000.0)
	fmt.Printf("    p95          : %6.2f\n", float64(p95.Microseconds())/1000.0)
	fmt.Printf("    p99          : %6.2f\n", float64(p99.Microseconds())/1000.0)
	fmt.Printf("    max          : %6.2f\n", float64(max.Microseconds())/1000.0)
	fmt.Println()
	successRate := 0.0
	if reqs > 0 {
		successRate = float64(reqs-errs) / float64(reqs)
	}
	fmt.Printf("  success        : [%s] %5.1f%%\n", asciiBar(successRate, 40), successRate*100)
	fmt.Println("  ─────────────────────────────────────────────────────────────────")
}

// --- helpers ---------------------------------------------------------

var authCredentials string

func dial(addr string) (*net.TCPConn, error) {
	taddr, err := net.ResolveTCPAddr("tcp", addr)
	if err != nil {
		return nil, err
	}
	conn, err := net.DialTCP("tcp", nil, taddr)
	if err != nil {
		return nil, err
	}
	_ = conn.SetNoDelay(true)
	_ = conn.SetKeepAlive(true)
	_ = conn.SetKeepAlivePeriod(2 * time.Minute)
	_ = conn.SetReadBuffer(256 << 10)
	_ = conn.SetWriteBuffer(256 << 10)
	if authCredentials != "" {
		if err := authenticate(conn); err != nil {
			conn.Close()
			return nil, err
		}
	}
	return conn, nil
}

func authenticate(conn *net.TCPConn) error {
	if _, err := conn.Write([]byte("AUTH " + authCredentials + "\n")); err != nil {
		return err
	}
	var reply []byte
	b := make([]byte, 1)
	for {
		if _, err := conn.Read(b); err != nil {
			return err
		}
		if b[0] == '\n' {
			break
		}
		reply = append(reply, b[0])
	}
	if strings.TrimSpace(string(reply)) != "OK" {
		return fmt.Errorf("AUTH failed: %s", reply)
	}
	return nil
}

func durStr(d time.Duration) string {
	if d == 0 {
		return "-"
	}
	ms := float64(d.Microseconds()) / 1000.0
	return fmt.Sprintf("%.2fms", ms)
}
