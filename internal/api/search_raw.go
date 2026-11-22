package api_storage

import (
	"bytes"
	"strings"
	"sync"
	"unicode/utf16"
	"unicode/utf8"

	"github.com/taymour/elysiandb/internal/docpath"
)

type rawSearch struct {
	b       []byte
	i       int
	pattern string
	buf     []byte
	lower   []byte
}

var (
	rawSearchPool = sync.Pool{New: func() any { return &rawSearch{} }}
	corePrefix    = []byte(docpath.CoreFieldsPrefix)
)

func SearchMatchesBytes(raw []byte, search string) (matched bool, ok bool) {
	return searchBytesLowered(raw, LoweredSearchPattern(search))
}

func LoweredSearchPattern(search string) string {
	return strings.ToLower(SearchPattern(search))
}

func searchBytesLowered(raw []byte, pattern string) (matched bool, ok bool) {
	s := rawSearchPool.Get().(*rawSearch)
	defer rawSearchPool.Put(s)

	s.b, s.i, s.pattern = raw, 0, pattern
	defer func() { s.b = nil }()

	s.ws()
	if s.i >= len(s.b) || s.b[s.i] != '{' {
		return false, false
	}

	matched, ok = s.value(true)
	if !ok || matched {
		return matched, ok
	}

	s.ws()

	return false, s.i == len(s.b)
}

func (s *rawSearch) ws() {
	for s.i < len(s.b) {
		switch s.b[s.i] {
		case ' ', '\t', '\n', '\r':
			s.i++
		default:
			return
		}
	}
}

func (s *rawSearch) value(walk bool) (bool, bool) {
	s.ws()
	if s.i >= len(s.b) {
		return false, false
	}

	switch c := s.b[s.i]; {
	case c == '{':
		return s.object(walk)
	case c == '[':
		return s.array(walk)
	case c == '"':
		str, ok := s.str()
		if !ok {
			return false, false
		}
		return walk && s.glob(str), true
	default:
		return false, s.literal()
	}
}

func (s *rawSearch) glob(str []byte) bool {
	ascii := true
	for _, c := range str {
		if c >= utf8.RuneSelf {
			ascii = false
			break
		}
	}

	if !ascii {
		return docpath.MatchGlobLower(s.pattern, strings.ToLower(string(str)))
	}

	lower := s.lower[:0]
	for _, c := range str {
		if 'A' <= c && c <= 'Z' {
			c += 'a' - 'A'
		}
		lower = append(lower, c)
	}
	s.lower = lower

	return docpath.MatchGlobLower(s.pattern, lower)
}

func searchableKey(key []byte) bool {
	return string(key) != "id" && string(key) != "@entity" && !bytes.HasPrefix(key, corePrefix)
}

func (s *rawSearch) object(walk bool) (bool, bool) {
	s.i++
	s.ws()
	if s.i < len(s.b) && s.b[s.i] == '}' {
		s.i++
		return false, true
	}

	for {
		s.ws()
		if s.i >= len(s.b) || s.b[s.i] != '"' {
			return false, false
		}

		key, ok := s.str()
		if !ok {
			return false, false
		}
		searchable := walk && searchableKey(key)

		s.ws()
		if s.i >= len(s.b) || s.b[s.i] != ':' {
			return false, false
		}
		s.i++

		matched, ok := s.value(searchable)
		if !ok {
			return false, false
		}
		if matched {
			return true, true
		}

		s.ws()
		if s.i >= len(s.b) {
			return false, false
		}

		switch s.b[s.i] {
		case ',':
			s.i++
		case '}':
			s.i++
			return false, true
		default:
			return false, false
		}
	}
}

func (s *rawSearch) array(walk bool) (bool, bool) {
	s.i++
	s.ws()
	if s.i < len(s.b) && s.b[s.i] == ']' {
		s.i++
		return false, true
	}

	for {
		matched, ok := s.value(walk)
		if !ok {
			return false, false
		}
		if matched {
			return true, true
		}

		s.ws()
		if s.i >= len(s.b) {
			return false, false
		}

		switch s.b[s.i] {
		case ',':
			s.i++
		case ']':
			s.i++
			return false, true
		default:
			return false, false
		}
	}
}

func (s *rawSearch) literal() bool {
	start := s.i
	for s.i < len(s.b) {
		switch s.b[s.i] {
		case ',', '}', ']', ' ', '\t', '\n', '\r':
			return s.i > start
		}
		s.i++
	}

	return s.i > start
}

func (s *rawSearch) str() ([]byte, bool) {
	s.i++
	start := s.i
	plain := true
	for s.i < len(s.b) {
		c := s.b[s.i]
		if c == '"' {
			break
		}
		if c == '\\' {
			plain = false
			s.i += 2
			continue
		}
		if c < 0x20 || c >= utf8.RuneSelf {
			plain = false
		}
		s.i++
	}

	if s.i >= len(s.b) {
		return nil, false
	}

	body := s.b[start:s.i]
	s.i++

	if plain {
		return body, true
	}

	return s.unquote(body)
}

func (s *rawSearch) unquote(body []byte) ([]byte, bool) {
	out := s.buf[:0]
	for r := 0; r < len(body); {
		c := body[r]
		switch {
		case c == '\\':
			if r+1 >= len(body) {
				return nil, false
			}
			switch body[r+1] {
			case '"', '\\', '/':
				out = append(out, body[r+1])
				r += 2
			case 'b':
				out = append(out, '\b')
				r += 2
			case 'f':
				out = append(out, '\f')
				r += 2
			case 'n':
				out = append(out, '\n')
				r += 2
			case 'r':
				out = append(out, '\r')
				r += 2
			case 't':
				out = append(out, '\t')
				r += 2
			case 'u':
				rr, ok := hex4(body, r+2)
				if !ok {
					return nil, false
				}
				r += 6
				if utf16.IsSurrogate(rr) {
					rr1, ok := rune(-1), false
					if r+6 <= len(body) && body[r] == '\\' && body[r+1] == 'u' {
						rr1, ok = hex4(body, r+2)
					}
					if dec := utf16.DecodeRune(rr, rr1); ok && dec != utf8.RuneError {
						r += 6
						out = utf8.AppendRune(out, dec)
						break
					}
					rr = utf8.RuneError
				}
				out = utf8.AppendRune(out, rr)
			default:
				return nil, false
			}
		case c < 0x20:
			return nil, false
		case c < utf8.RuneSelf:
			out = append(out, c)
			r++
		default:
			rr, size := utf8.DecodeRune(body[r:])
			r += size
			out = utf8.AppendRune(out, rr)
		}
	}

	s.buf = out

	return out, true
}

func hex4(b []byte, at int) (rune, bool) {
	if at+4 > len(b) {
		return 0, false
	}

	var r rune
	for _, c := range b[at : at+4] {
		switch {
		case '0' <= c && c <= '9':
			c -= '0'
		case 'a' <= c && c <= 'f':
			c = c - 'a' + 10
		case 'A' <= c && c <= 'F':
			c = c - 'A' + 10
		default:
			return 0, false
		}
		r = r*16 + rune(c)
	}

	return r, true
}
