package parsing_test

import (
	"bytes"
	"testing"

	"github.com/taymour/elysiandb/internal/transport/tcp/parsing"
)

func TestFirstWordBytesBasic(t *testing.T) {
	cmd, rest := parsing.FirstWordBytes([]byte("PING data"))
	if !bytes.Equal(cmd, []byte("PING")) {
		t.Fatal("bad cmd")
	}
	if !bytes.Equal(rest, []byte("data")) {
		t.Fatal("bad rest")
	}
}

func TestFirstWordBytesTrim(t *testing.T) {
	cmd, rest := parsing.FirstWordBytes([]byte("  GET key\r\n"))
	if !bytes.Equal(cmd, []byte("GET")) {
		t.Fatal("bad cmd")
	}
	if !bytes.Equal(rest, []byte("key")) {
		t.Fatal("bad rest")
	}
}

func TestFirstWordBytesOnlyCmd(t *testing.T) {
	cmd, rest := parsing.FirstWordBytes([]byte("RESET\n"))
	if !bytes.Equal(cmd, []byte("RESET")) {
		t.Fatal("bad cmd")
	}
	if len(rest) != 0 {
		t.Fatal("rest not empty")
	}
}

func TestFirstWordBytesEmpty(t *testing.T) {
	cmd, rest := parsing.FirstWordBytes([]byte(""))
	if len(cmd) != 0 || len(rest) != 0 {
		t.Fatal("expected empty")
	}
}

func TestEqASCIITrue(t *testing.T) {
	if !parsing.EqASCII([]byte("ping"), []byte("PING")) {
		t.Fatal("should match")
	}
}

func TestEqASCIIFalseLength(t *testing.T) {
	if parsing.EqASCII([]byte("PING"), []byte("PIN")) {
		t.Fatal("should fail")
	}
}

func TestEqASCIIFalseValue(t *testing.T) {
	if parsing.EqASCII([]byte("PING"), []byte("PONG")) {
		t.Fatal("should fail")
	}
}

func TestParseDecimalBytesValid(t *testing.T) {
	n, err := parsing.ParseDecimalBytes([]byte("12345"))
	if err != nil || n != 12345 {
		t.Fatal("bad parse")
	}
}

func TestParseDecimalBytesStops(t *testing.T) {
	n, err := parsing.ParseDecimalBytes([]byte("123abc"))
	if err != nil || n != 123 {
		t.Fatal("bad parse")
	}
}

func TestParseDecimalBytesEmpty(t *testing.T) {
	if _, err := parsing.ParseDecimalBytes([]byte("")); err == nil {
		t.Fatal("should error")
	}
}

func TestParseDecimalBytesNoDigits(t *testing.T) {
	if _, err := parsing.ParseDecimalBytes([]byte("abc")); err == nil {
		t.Fatal("should error")
	}
}

func TestParseDecimalBytesOverflow(t *testing.T) {
	max := []byte("92233720368547758070")
	if _, err := parsing.ParseDecimalBytes(max); err == nil {
		t.Fatal("should overflow")
	}
}

func TestJoinByteSlicesBasic(t *testing.T) {
	out := parsing.JoinByteSlices([][]byte{
		[]byte("a"),
		[]byte("b"),
		[]byte("c"),
	}, []byte(","))
	if !bytes.Equal(out, []byte("a,b,c")) {
		t.Fatal("bad join")
	}
}

func TestJoinByteSlicesEmpty(t *testing.T) {
	out := parsing.JoinByteSlices(nil, []byte(","))
	if len(out) != 0 {
		t.Fatal("should be empty")
	}
}

func TestJoinByteSlicesSingle(t *testing.T) {
	out := parsing.JoinByteSlices([][]byte{[]byte("x")}, []byte(","))
	if !bytes.Equal(out, []byte("x")) {
		t.Fatal("bad single")
	}
}
