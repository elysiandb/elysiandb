package engine

import "sync"

type Arena struct {
	mu        sync.Mutex
	chunks    [][]byte
	cur       int
	offset    int
	chunkSize int
}

func NewArena(chunkSize int) *Arena {
	if chunkSize <= 0 {
		chunkSize = 1 << 20
	}

	a := &Arena{
		chunkSize: chunkSize,
	}

	a.chunks = append(a.chunks, make([]byte, chunkSize))

	return a
}

func (a *Arena) Alloc(n int) []byte {
	if n <= 0 {
		return nil
	}

	a.mu.Lock()
	defer a.mu.Unlock()

	if n > a.chunkSize {
		buf := make([]byte, n)
		a.chunks = append(a.chunks, buf)
		return buf[:n]
	}

	curChunk := a.chunks[a.cur]

	if a.offset+n > len(curChunk) {
		buf := make([]byte, a.chunkSize)
		a.chunks = append(a.chunks, buf)
		a.cur++
		a.offset = 0
		curChunk = buf
	}

	start := a.offset
	a.offset += n

	return curChunk[start:a.offset]
}

func (a *Arena) Reset() {
	a.mu.Lock()
	if len(a.chunks) == 0 {
		a.chunks = append(a.chunks, make([]byte, a.chunkSize))
		a.cur = 0
		a.offset = 0
		a.mu.Unlock()
		return
	}

	for i := 1; i < len(a.chunks); i++ {
		a.chunks[i] = nil
	}

	a.chunks = a.chunks[:1]
	a.cur = 0
	a.offset = 0
	a.mu.Unlock()
}

func (a *Arena) CurChunkCount() int {
	a.mu.Lock()
	n := len(a.chunks)
	a.mu.Unlock()

	return n
}
