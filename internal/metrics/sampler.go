package metrics

import (
	"math"
	"os"
	"path/filepath"
	"runtime"
	"runtime/metrics"
	"sort"
	"strconv"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

type Sources struct {
	DataFolder           func() string
	KVKeys               func() (uint64, uint64)
	EntityCounts         func() map[string]int
	IndexBacklog         func() int
	CacheEntries         func() int
	RecoverySizes        func() (int64, int64)
	AdminSessions        func() int
	Transactions         func() (int, time.Duration)
	HTTPConnections      func() int
	MongoPing            func() (time.Duration, error)
	MongoPoolInUse       func() int
	DefaultAdminPassword func() bool
}

type resolution struct {
	name   string
	step   time.Duration
	points []Point
	next   int
	size   int
	prev   *snapshot
}

type snapshot struct {
	at          time.Time
	counters    [numCounters]uint64
	http        hist
	tcp         hist
	hook        hist
	logWarnings uint64
	logErrors   uint64
	gcCycles    uint64
	gcPauses    []uint64
	cpuSeconds  float64
	procs       int
	gauges      map[int]float64
}

type slowValues struct {
	entityCounts   map[string]int
	recoveryJSON   float64
	recoveryStore  float64
	openFDs        float64
	adminSessions  float64
	diskBytes      float64
	mongoPingMs    float64
	mongoErr       error
	defaultAdminPw bool
}

type sampler struct {
	mu          sync.RWMutex
	sources     Sources
	resolutions []*resolution
	recent      []*snapshot
	routeMarks  []map[string]routeSnapshot
	slow        slowValues
	ticks       int64
	running     bool
	stop        chan struct{}
	problems    *problemTracker
	backlogFrom time.Time
}

const recentSnapshots = 301

var (
	state = newSampler()

	gcPauseBuckets []float64
)

func newSampler() *sampler {
	return &sampler{
		resolutions: []*resolution{
			{name: "15m", step: time.Second, points: make([]Point, 900)},
			{name: "1h", step: 10 * time.Second, points: make([]Point, 1080)},
			{name: "24h", step: time.Minute, points: make([]Point, 1440)},
		},
		slow: slowValues{
			entityCounts:  map[string]int{},
			recoveryJSON:  math.NaN(),
			recoveryStore: math.NaN(),
			openFDs:       math.NaN(),
			adminSessions: math.NaN(),
			diskBytes:     math.NaN(),
			mongoPingMs:   math.NaN(),
		},
		problems: newProblemTracker(),
	}
}

func SetSources(s Sources) {
	state.mu.Lock()
	state.sources = s
	state.mu.Unlock()
}

func Start() {
	state.mu.Lock()
	if state.running {
		state.mu.Unlock()
		return
	}
	state.running = true
	state.stop = make(chan struct{})
	stop := state.stop
	state.mu.Unlock()

	state.tick(time.Now())

	go func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()

		for {
			select {
			case <-stop:
				return
			case now := <-ticker.C:
				state.tick(now)
			}
		}
	}()
}

func Stop() {
	state.mu.Lock()
	defer state.mu.Unlock()

	if !state.running {
		return
	}

	close(state.stop)
	state.running = false
}

func Reset() {
	Stop()
	state = newSampler()
	for i := range counters {
		counters[i].Store(0)
	}
	httpLatency = Histogram{}
	tcpLatency = Histogram{}
	hookLatency = Histogram{}
	tcpOpen.Store(0)
	routeStats = sync.Map{}
	hookStats = sync.Map{}
	authFailures = newAddressCounter()
	persistence = &persistenceState{}
	recoveryState = &timedError{}
	hookFailure = &timedError{}
	replayed.Store(0)
	startedAt = time.Now()
}

func Tick(now time.Time) {
	state.tick(now)
}

func (s *sampler) tick(now time.Time) {
	s.mu.RLock()
	sources := s.sources
	ticks := s.ticks
	s.mu.RUnlock()

	slow := s.refreshSlow(sources, ticks)
	cur := takeSnapshot(now, sources, slow)

	s.mu.Lock()
	s.slow = slow
	s.ticks++

	for _, r := range s.resolutions {
		if r.prev == nil {
			r.prev = cur
			continue
		}

		if now.Sub(r.prev.at) >= r.step-r.step/10 {
			r.push(derive(r.prev, cur))
			r.prev = cur
		}
	}

	s.recent = append(s.recent, cur)
	if len(s.recent) > recentSnapshots {
		s.recent = s.recent[len(s.recent)-recentSnapshots:]
	}

	if ticks%60 == 0 {
		s.routeMarks = append(s.routeMarks, snapshotRoutes())
		if len(s.routeMarks) > 16 {
			s.routeMarks = s.routeMarks[len(s.routeMarks)-16:]
		}
	}

	backlog := cur.gauges[sIndexBacklog]
	if backlog > 0 {
		if s.backlogFrom.IsZero() {
			s.backlogFrom = now
		}
	} else {
		s.backlogFrom = time.Time{}
	}
	s.mu.Unlock()

	s.problems.update(now, s.evaluate(now))
}

func (r *resolution) push(p Point) {
	r.points[r.next] = p
	r.next = (r.next + 1) % len(r.points)
	if r.size < len(r.points) {
		r.size++
	}
}

func (r *resolution) ordered() []Point {
	out := make([]Point, 0, r.size)
	start := (r.next - r.size + len(r.points)) % len(r.points)
	for i := 0; i < r.size; i++ {
		out = append(out, r.points[(start+i)%len(r.points)])
	}

	return out
}

func (s *sampler) refreshSlow(src Sources, ticks int64) slowValues {
	s.mu.RLock()
	slow := s.slow
	s.mu.RUnlock()

	if ticks%10 == 0 {
		if src.EntityCounts != nil {
			slow.entityCounts = src.EntityCounts()
		}
		if src.RecoverySizes != nil {
			j, st := src.RecoverySizes()
			slow.recoveryJSON, slow.recoveryStore = float64(j), float64(st)
		}
		if src.AdminSessions != nil {
			slow.adminSessions = float64(src.AdminSessions())
		}
		if src.MongoPing != nil {
			d, err := src.MongoPing()
			slow.mongoErr = err
			slow.mongoPingMs = math.NaN()
			if err == nil {
				slow.mongoPingMs = float64(d) / float64(time.Millisecond)
			}
		}
		slow.openFDs = openFileDescriptors()
	}

	if ticks%60 == 0 {
		if src.DataFolder != nil {
			slow.diskBytes = folderSize(src.DataFolder())
		}
		if src.DefaultAdminPassword != nil {
			slow.defaultAdminPw = src.DefaultAdminPassword()
		}
	}

	return slow
}

var runtimeSamples = []metrics.Sample{
	{Name: "/memory/classes/heap/objects:bytes"},
	{Name: "/memory/classes/total:bytes"},
	{Name: "/sched/goroutines:goroutines"},
	{Name: "/gc/cycles/total:gc-cycles"},
	{Name: "/sched/pauses/total/gc:seconds"},
}

var runtimeMu sync.Mutex

func takeSnapshot(now time.Time, src Sources, slow slowValues) *snapshot {
	s := &snapshot{
		at:     now,
		http:   httpLatency.snapshot(),
		tcp:    tcpLatency.snapshot(),
		hook:   hookLatency.snapshot(),
		procs:  runtime.GOMAXPROCS(0),
		gauges: map[int]float64{},
	}

	for i := range counters {
		s.counters[i] = counters[i].Load()
	}

	s.logWarnings, s.logErrors = log.Counts()
	s.cpuSeconds = processCPUSeconds()

	runtimeMu.Lock()
	metrics.Read(runtimeSamples)
	for _, sample := range runtimeSamples {
		switch sample.Name {
		case "/memory/classes/heap/objects:bytes":
			s.gauges[sHeapBytes] = uintValue(sample.Value)
		case "/memory/classes/total:bytes":
			s.gauges[sTotalBytes] = uintValue(sample.Value)
		case "/sched/goroutines:goroutines":
			s.gauges[sGoroutines] = uintValue(sample.Value)
		case "/gc/cycles/total:gc-cycles":
			s.gcCycles = uint64(uintValue(sample.Value))
		case "/sched/pauses/total/gc:seconds":
			if sample.Value.Kind() == metrics.KindFloat64Histogram {
				h := sample.Value.Float64Histogram()
				s.gcPauses = append([]uint64(nil), h.Counts...)
				if gcPauseBuckets == nil {
					gcPauseBuckets = append([]float64(nil), h.Buckets...)
				}
			}
		}
	}
	runtimeMu.Unlock()

	if src.KVKeys != nil {
		keys, ttl := src.KVKeys()
		s.gauges[sKVKeys] = float64(keys)
		s.gauges[sKVKeysTTL] = float64(ttl)
	}

	total := 0
	for _, c := range slow.entityCounts {
		total += c
	}
	s.gauges[sEntities] = float64(total)

	if src.IndexBacklog != nil {
		s.gauges[sIndexBacklog] = float64(src.IndexBacklog())
	}
	if src.CacheEntries != nil {
		s.gauges[sCacheEntries] = float64(src.CacheEntries())
	}
	if src.HTTPConnections != nil {
		s.gauges[sHTTPConns] = float64(src.HTTPConnections())
	}
	if src.Transactions != nil {
		open, oldest := src.Transactions()
		s.gauges[sOpenTransactions] = float64(open)
		s.gauges[sOldestTransaction] = oldest.Seconds()
	}
	if src.MongoPoolInUse != nil {
		s.gauges[sMongoPoolInUse] = float64(src.MongoPoolInUse())
	}

	s.gauges[sTCPConns] = float64(tcpOpen.Load())
	s.gauges[sRecoveryJSONBytes] = slow.recoveryJSON
	s.gauges[sRecoveryStoreBytes] = slow.recoveryStore
	s.gauges[sOpenFDs] = slow.openFDs
	s.gauges[sAdminSessions] = slow.adminSessions
	s.gauges[sDiskBytes] = slow.diskBytes
	s.gauges[sMongoPingMs] = slow.mongoPingMs

	return s
}

func uintValue(v metrics.Value) float64 {
	if v.Kind() == metrics.KindUint64 {
		return float64(v.Uint64())
	}

	if v.Kind() == metrics.KindFloat64 {
		return v.Float64()
	}

	return math.NaN()
}

func openFileDescriptors() float64 {
	entries, err := os.ReadDir("/proc/self/fd")
	if err != nil {
		return math.NaN()
	}

	return float64(len(entries))
}

func folderSize(folder string) float64 {
	if folder == "" {
		return math.NaN()
	}

	var total int64
	err := filepath.WalkDir(folder, func(_ string, d os.DirEntry, err error) error {
		if err != nil {
			return nil
		}
		if d.Type().IsRegular() {
			if info, err := d.Info(); err == nil {
				total += info.Size()
			}
		}
		return nil
	})
	if err != nil {
		return math.NaN()
	}

	return float64(total)
}

type SeriesResponse struct {
	Range         string
	StepSeconds   int
	SlowRequestMs int
	Points        []Point
}

func Series(rangeName string, since int64) (SeriesResponse, bool) {
	state.mu.RLock()
	defer state.mu.RUnlock()

	for _, r := range state.resolutions {
		if r.name != rangeName {
			continue
		}

		points := r.ordered()
		if since > 0 {
			i := sort.Search(len(points), func(i int) bool { return points[i].T > since })
			points = points[i:]
		}

		return SeriesResponse{Range: r.name, StepSeconds: int(r.step / time.Second), Points: points}, true
	}

	return SeriesResponse{}, false
}

func (s SeriesResponse) MarshalJSON() ([]byte, error) {
	b := make([]byte, 0, 64+len(s.Points)*numSeries*8)
	b = append(b, `{"range":`...)
	b = strconv.AppendQuote(b, s.Range)
	b = append(b, `,"step_seconds":`...)
	b = strconv.AppendInt(b, int64(s.StepSeconds), 10)
	b = append(b, `,"slow_request_ms":`...)
	b = strconv.AppendInt(b, int64(s.SlowRequestMs), 10)
	b = append(b, `,"time":[`...)
	for i, p := range s.Points {
		if i > 0 {
			b = append(b, ',')
		}
		b = strconv.AppendInt(b, p.T, 10)
	}
	b = append(b, `],"series":{`...)
	for si, name := range seriesNames {
		if si > 0 {
			b = append(b, ',')
		}
		b = strconv.AppendQuote(b, name)
		b = append(b, ":["...)
		for i, p := range s.Points {
			if i > 0 {
				b = append(b, ',')
			}
			b = appendNumber(b, p.V[si])
		}
		b = append(b, ']')
	}
	b = append(b, "}}"...)

	return b, nil
}

func appendNumber(b []byte, v float64) []byte {
	if math.IsNaN(v) || math.IsInf(v, 0) {
		return append(b, "null"...)
	}

	return strconv.AppendFloat(b, round(v), 'f', -1, 64)
}

func (s *sampler) window(d time.Duration) (*snapshot, *snapshot) {
	if len(s.recent) < 2 {
		return nil, nil
	}

	cur := s.recent[len(s.recent)-1]
	target := cur.at.Add(-d)
	for _, snap := range s.recent {
		if !snap.at.Before(target) {
			if snap == cur {
				return nil, nil
			}
			return snap, cur
		}
	}

	return s.recent[0], cur
}
