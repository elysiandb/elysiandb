package hook

import (
	"math"
	"strings"
	"sync"
)

type vkind uint8

const (
	vUndef vkind = iota
	vNull
	vBool
	vNum
	vStr
	vTmp
	vArr
	vOpaque
)

type val struct {
	n  float64
	id uint32
	k  vkind
}

var (
	undefVal = val{k: vUndef}
	nullVal  = val{k: vNull}
)

func numVal(n float64) val {
	return val{k: vNum, n: n}
}

func boolVal(b bool) val {
	if b {
		return val{k: vBool, n: 1}
	}

	return val{k: vBool}
}

type frame struct {
	row     []val
	regs    []val
	local   []val
	lits    []val
	free    []val
	tmp     []string
	strs    []string
	ret     val
	bail    bool
	touched bool
}

func (fr *frame) str(v val) string {
	if v.k == vStr {
		return fr.strs[v.id]
	}

	return fr.tmp[v.id]
}

func isString(v val) bool {
	return v.k == vStr || v.k == vTmp
}

func (fr *frame) truthy(v val) bool {
	switch v.k {
	case vUndef, vNull:
		return false
	case vBool:
		return v.n != 0
	case vNum:
		return v.n != 0 && v.n == v.n
	case vStr:
		return v.id != 0
	case vTmp:
		return fr.tmp[v.id] != ""
	case vArr:
		return true
	}

	fr.bail = true

	return false
}

func (fr *frame) strictEq(a, b val) bool {
	if a.k == b.k {
		switch a.k {
		case vNum, vBool:
			return a.n == b.n
		case vStr:
			return a.id == b.id
		case vTmp:
			return fr.tmp[a.id] == fr.tmp[b.id]
		case vUndef, vNull:
			return true
		}

		fr.bail = true

		return false
	}

	if isString(a) && isString(b) {
		return fr.str(a) == fr.str(b)
	}

	if a.k == vArr || b.k == vArr || a.k == vOpaque || b.k == vOpaque {
		fr.bail = true
	}

	return false
}

func (fr *frame) looseEq(a, b val) bool {
	aNull := a.k == vUndef || a.k == vNull
	bNull := b.k == vUndef || b.k == vNull
	if aNull || bNull {
		if a.k == vArr || b.k == vArr || a.k == vOpaque || b.k == vOpaque {
			fr.bail = a.k == vOpaque || b.k == vOpaque
			return false
		}
		return aNull && bNull
	}

	if a.k == b.k || (isString(a) && isString(b)) {
		return fr.strictEq(a, b)
	}

	fr.bail = true

	return false
}

func asciiOnly(s string) bool {
	for i := 0; i < len(s); i++ {
		if s[i] >= 0x80 {
			return false
		}
	}

	return true
}

func (fr *frame) typeName(v val) val {
	name := ""
	switch v.k {
	case vUndef:
		name = "undefined"
	case vBool:
		name = "boolean"
	case vNum:
		name = "number"
	case vStr, vTmp:
		name = "string"
	case vNull, vArr:
		name = "object"
	default:
		fr.bail = true
		return undefVal
	}

	fr.tmp = append(fr.tmp, name)

	return val{k: vTmp, id: uint32(len(fr.tmp) - 1)}
}

func (fr *frame) split(v val, sep string) val {
	if !isString(v) {
		fr.bail = true
		return undefVal
	}

	s := fr.str(v)
	start := len(fr.tmp)
	for {
		i := strings.Index(s, sep)
		if i < 0 {
			break
		}
		fr.tmp = append(fr.tmp, s[:i])
		s = s[i+len(sep):]
	}
	fr.tmp = append(fr.tmp, s)

	return val{k: vArr, id: uint32(start), n: float64(len(fr.tmp) - start)}
}

func (fr *frame) element(a val, i int) val {
	if a.k != vArr {
		fr.bail = true
		return undefVal
	}

	if i < 0 || i >= int(a.n) {
		return undefVal
	}

	return val{k: vTmp, id: a.id + uint32(i)}
}

const maxParsedDigits = 15

func (fr *frame) parseInt10(v val) val {
	if !isString(v) {
		fr.bail = true
		return undefVal
	}

	s := fr.str(v)
	n := 0.0
	digits := 0
	for digits < len(s) && s[digits] >= '0' && s[digits] <= '9' {
		n = n*10 + float64(s[digits]-'0')
		digits++
	}

	if digits == 0 || digits > maxParsedDigits {
		fr.bail = true
		return undefVal
	}

	return numVal(n)
}

func (fr *frame) length(v val) val {
	switch {
	case v.k == vArr:
		return numVal(v.n)
	case isString(v):
		s := fr.str(v)
		if asciiOnly(s) {
			return numVal(float64(len(s)))
		}
	}

	fr.bail = true

	return undefVal
}

func (fr *frame) arith(op byte, a, b val) val {
	if a.k != vNum || b.k != vNum {
		fr.bail = true
		return undefVal
	}

	switch op {
	case '+':
		return numVal(a.n + b.n)
	case '-':
		return numVal(a.n - b.n)
	case '*':
		return numVal(a.n * b.n)
	case '/':
		return numVal(a.n / b.n)
	}

	return numVal(math.Mod(a.n, b.n))
}

func (fr *frame) less(a, b val, strict, swap bool) bool {
	switch {
	case a.k == vNum && b.k == vNum:
	case isString(a) && isString(b):
		x, y := fr.str(a), fr.str(b)
		if !asciiOnly(x) || !asciiOnly(y) {
			fr.bail = true
			return false
		}
		if swap {
			x, y = y, x
		}
		if strict {
			return x < y
		}
		return x <= y
	default:
		fr.bail = true
		return false
	}

	x, y := a.n, b.n
	if swap {
		x, y = y, x
	}
	if strict {
		return x < y
	}

	return x <= y
}

type ctl uint8

const (
	ctlNext ctl = iota
	ctlContinue
	ctlReturn
	ctlBail
)

type expr func(fr *frame) val

type stmt func(fr *frame) ctl

var framePool = sync.Pool{New: func() any { return &frame{} }}

func sizedVals(s []val, n int) []val {
	if cap(s) < n {
		return make([]val, n)
	}

	s = s[:n]
	clear(s)

	return s
}

func acquireFrame(regs, locals, lits, free int, strs []string) *frame {
	fr := framePool.Get().(*frame)
	fr.regs = sizedVals(fr.regs, regs)
	fr.local = sizedVals(fr.local, locals)
	fr.lits = sizedVals(fr.lits, lits)
	fr.free = sizedVals(fr.free, free)
	fr.tmp = fr.tmp[:0]
	fr.strs = strs
	fr.bail = false
	fr.touched = false
	fr.row = nil

	return fr
}

func releaseFrame(fr *frame) {
	clear(fr.tmp[:cap(fr.tmp)])
	fr.tmp = fr.tmp[:0]
	fr.strs = nil
	fr.row = nil
	framePool.Put(fr)
}
