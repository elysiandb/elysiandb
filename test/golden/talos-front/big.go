package main

import (
	"encoding/hex"
	"math"
	"math/big"
	"strconv"

	"github.com/taymour/elysiandb/test/golden/goldenio"
)

func pow(base int64, e uint) *big.Int {
	return new(big.Int).Exp(big.NewInt(base), big.NewInt(int64(e)), nil)
}

func add(x, y *big.Int) *big.Int {
	return new(big.Int).Add(x, y)
}

func neg(x *big.Int) *big.Int {
	return new(big.Int).Neg(x)
}

func bigCorpus() []*big.Int {
	strs := []string{
		"0", "1", "-1", "2", "-2", "3", "-3", "7", "10", "-10", "255", "256", "-256", "65535", "65536",
		"2147483647", "2147483648", "-2147483648", "-2147483649",
		"4294967295", "4294967296", "-4294967296", "-4294967297",
		"9007199254740991", "9007199254740992", "9007199254740993", "-9007199254740993",
		"4611686018427387904", "9223372036854775807", "9223372036854775808", "9223372036854775809",
		"-9223372036854775807", "-9223372036854775808", "-9223372036854775809",
		"18446744073709551615", "18446744073709551616", "18446744073709551617",
		"-18446744073709551615", "-18446744073709551616", "-18446744073709551617",
		"36893488147419103232", "170141183460469231731687303715884105727",
		"340282366920938463463374607431768211455", "340282366920938463463374607431768211456",
		"-340282366920938463463374607431768211456",
		"1000000000000000000000000000000", "-1000000000000000000000000000000",
		"0xdeadbeefcafebabe1234567890abcdef", "-0xfedcba9876543210fedcba9876543210ff",
	}
	var out []*big.Int
	for _, s := range strs {
		v, ok := new(big.Int).SetString(s, 0)
		if !ok {
			panic(s)
		}
		out = append(out, v)
	}
	two1000 := pow(2, 1000)
	two1024 := pow(2, 1024)
	out = append(out,
		pow(3, 100),
		neg(pow(3, 100)),
		two1000,
		add(two1000, big.NewInt(-1)),
		neg(two1000),
		add(neg(two1000), big.NewInt(1)),
		add(pow(10, 100), big.NewInt(7)),
		add(add(pow(2, 999), pow(2, 500)), big.NewInt(1)),
		new(big.Int).Sub(two1024, pow(2, 970)),
		new(big.Int).Sub(two1024, pow(2, 971)),
		neg(new(big.Int).Sub(two1024, pow(2, 970))),
		two1024,
	)
	return out
}

func roundingCorpus() []*big.Int {
	var out []*big.Int
	two53 := pow(2, 53)
	two54 := pow(2, 54)
	for i := int64(-6); i <= 6; i++ {
		out = append(out, add(two53, big.NewInt(i)), add(two54, big.NewInt(i)), neg(add(two54, big.NewInt(i))))
	}
	for _, e := range []uint{60, 63, 64, 65, 100, 200, 1000, 1023} {
		p := pow(2, e)
		for _, d := range []int64{-3, -2, -1, 0, 1, 2, 3} {
			shift := int(e) - 53
			delta := new(big.Int).Lsh(big.NewInt(d), uint(shift))
			if shift > 1 {
				out = append(out, add(p, add(delta, new(big.Int).Lsh(big.NewInt(1), uint(shift-1)))))
				out = append(out, add(p, add(delta, new(big.Int).Lsh(big.NewInt(1), uint(shift-2)))))
			}
			out = append(out, add(p, delta), add(p, big.NewInt(d)))
		}
	}
	two1024 := pow(2, 1024)
	for _, d := range []int64{969, 970, 971, 972} {
		out = append(out, new(big.Int).Sub(two1024, pow(2, uint(d))))
		out = append(out, add(new(big.Int).Sub(two1024, pow(2, uint(d))), big.NewInt(1)))
	}
	out = append(out, pow(2, 1025), neg(pow(2, 1025)), pow(10, 400), neg(pow(10, 400)))
	return out
}

func accInt(a big.Accuracy) int {
	return int(a)
}

func tryOp(f func() *big.Int) any {
	var res any
	func() {
		defer func() {
			if r := recover(); r != nil {
				res = map[string]any{"panic": true}
			}
		}()
		z := f()
		if z == nil {
			res = nil
		} else {
			res = z.Text(16)
		}
	}()
	return res
}

func genBig(out string) error {
	values := bigCorpus()
	var lines goldenio.Lines
	for i, v := range values {
		texts := map[string]string{}
		for b := 2; b <= 62; b++ {
			texts[strconv.Itoa(b)] = v.Text(b)
		}
		f, acc := v.Float64()
		lines.Add(map[string]any{
			"i":       i,
			"dec":     v.String(),
			"sign":    v.Sign(),
			"bitlen":  v.BitLen(),
			"tz":      v.TrailingZeroBits(),
			"int64":   strconv.FormatInt(v.Int64(), 10),
			"uint64":  strconv.FormatUint(v.Uint64(), 10),
			"isint64": v.IsInt64(),
			"isuint":  v.IsUint64(),
			"text":    texts,
			"bytes":   hex.EncodeToString(v.Bytes()),
			"f64":     fbits(f),
			"acc":     accInt(acc),
			"neg":     new(big.Int).Neg(v).Text(16),
			"not":     new(big.Int).Not(v).Text(16),
			"abs":     new(big.Int).Abs(v).Text(16),
		})
	}
	if err := lines.Write(out, "big/values.jsonl"); err != nil {
		return err
	}

	var bin goldenio.Lines
	for i, x := range values {
		for j, y := range values {
			bin.Add(map[string]any{
				"x":      i,
				"y":      j,
				"add":    new(big.Int).Add(x, y).Text(16),
				"sub":    new(big.Int).Sub(x, y).Text(16),
				"mul":    new(big.Int).Mul(x, y).Text(16),
				"quo":    tryOp(func() *big.Int { return new(big.Int).Quo(x, y) }),
				"rem":    tryOp(func() *big.Int { return new(big.Int).Rem(x, y) }),
				"div":    tryOp(func() *big.Int { return new(big.Int).Div(x, y) }),
				"mod":    tryOp(func() *big.Int { return new(big.Int).Mod(x, y) }),
				"and":    new(big.Int).And(x, y).Text(16),
				"or":     new(big.Int).Or(x, y).Text(16),
				"xor":    new(big.Int).Xor(x, y).Text(16),
				"andnot": new(big.Int).AndNot(x, y).Text(16),
				"cmp":    x.Cmp(y),
				"cmpabs": x.CmpAbs(y),
			})
		}
	}
	if err := bin.Write(out, "big/binary.jsonl"); err != nil {
		return err
	}

	var sh goldenio.Lines
	for i, x := range values {
		for _, n := range []uint{0, 1, 2, 3, 7, 8, 31, 32, 33, 63, 64, 65, 127, 128, 129, 200, 1000, 1025} {
			sh.Add(map[string]any{
				"x":   i,
				"n":   n,
				"lsh": new(big.Int).Lsh(x, n).Text(16),
				"rsh": new(big.Int).Rsh(x, n).Text(16),
			})
		}
	}
	if err := sh.Write(out, "big/shift.jsonl"); err != nil {
		return err
	}

	var ex goldenio.Lines
	bases := []*big.Int{big.NewInt(0), big.NewInt(1), big.NewInt(-1), big.NewInt(2), big.NewInt(-2), big.NewInt(3), big.NewInt(-3), big.NewInt(10), big.NewInt(12345), pow(2, 64), neg(pow(2, 63)), pow(3, 40)}
	exps := []*big.Int{big.NewInt(-2), big.NewInt(-1), big.NewInt(0), big.NewInt(1), big.NewInt(2), big.NewInt(3), big.NewInt(10), big.NewInt(63), big.NewInt(64), big.NewInt(100), big.NewInt(257)}
	mods := []*big.Int{nil, big.NewInt(0), big.NewInt(1), big.NewInt(-1), big.NewInt(2), big.NewInt(7), big.NewInt(-7), big.NewInt(1000000007), add(pow(2, 64), big.NewInt(13)), pow(2, 100), big.NewInt(12)}
	for _, x := range bases {
		for _, y := range exps {
			for _, m := range mods {
				var ms any
				if m != nil {
					ms = m.String()
				}
				var zs any
				z := new(big.Int).Exp(x, y, m)
				if z != nil {
					zs = z.String()
				}
				if m == nil && y.Sign() > 0 && y.Int64() > 100 && x.BitLen() > 2 {
					continue
				}
				ex.Add(map[string]any{"x": x.String(), "y": y.String(), "m": ms, "z": zs})
			}
		}
	}
	if err := ex.Write(out, "big/exp.jsonl"); err != nil {
		return err
	}

	var ss goldenio.Lines
	setStrings := []string{
		"", "+", "-", "0", "-0", "+0", "00", "007", "08", "0x", "0X", "0b", "0o", "0x0", "0x1F", "0XfF", "0b101", "0B11", "0o17", "0O17", "017",
		"0_1", "0x_1", "0b_1", "0o_7", "0_7", "1_000", "1__000", "_1", "1_", "0x1_", "0x1__2", "0xg", "0b2", "0o8", "09",
		"123", "-123", "+123", "123abc", "abc", "ABC", "zz", "ZZ", "zZ", "Zz", "-zz",
		"18446744073709551616", "-18446744073709551617", "0xffffffffffffffffffffffffffffffff", "1e3", "1.5", " 1", "1 ", "--1", "+-1",
		"9223372036854775808", "0x8000000000000000", "0b1111111111111111111111111111111111111111111111111111111111111111",
		"340282366920938463463374607431768211456", "1_2_3_4_5_6_7_8_9_0_1_2_3_4_5_6_7_8_9_0", "0xabcdef0123456789abcdef0123",
		"١", "1\x00", "١",
	}
	bases2 := []int{0, 2, 8, 10, 16, 36, 37, 62}
	for _, s := range setStrings {
		for _, b := range bases2 {
			v, ok := new(big.Int).SetString(s, b)
			m := map[string]any{"s": s, "base": b, "ok": ok}
			if ok {
				m["v"] = v.String()
			}
			ss.Add(m)
		}
	}
	if err := ss.Write(out, "big/setstring.jsonl"); err != nil {
		return err
	}

	floats := []float64{
		0, math.Copysign(0, -1), 1, -1, 0.5, -0.5, 1.5, -1.5, 2.5, 3, -3, 1e15 + 0.5, 4503599627370495.5, 4503599627370496,
		9007199254740991, 9007199254740992, 9007199254740994, -9007199254740994,
		math.Ldexp(1, 63), -math.Ldexp(1, 63), math.Ldexp(1, 64), -math.Ldexp(1, 64), math.Ldexp(1, 100), 1e300, -1e300,
		math.MaxFloat64, -math.MaxFloat64, math.SmallestNonzeroFloat64, -math.SmallestNonzeroFloat64,
		math.Float64frombits(0x000fffffffffffff), math.Float64frombits(0x0010000000000000), math.Inf(1), math.Inf(-1),
		0.1, 123456789.125, -123456789.75, 1e-300, 4.9406564584124654e-320, 1e21, 1.2345678901234567e19,
	}
	var ff goldenio.Lines
	for _, f := range floats {
		bf := big.NewFloat(f)
		n, acc := bf.Int(nil)
		var ns any
		if n != nil {
			ns = n.String()
		}
		ff.Add(map[string]any{"f": fbits(f), "isint": bf.IsInt(), "int": ns, "acc": accInt(acc)})
	}
	if err := ff.Write(out, "big/from_float.jsonl"); err != nil {
		return err
	}

	var cf goldenio.Lines
	for i, x := range values {
		for _, f := range floats {
			cf.Add(map[string]any{"x": i, "f": fbits(f), "cmp": new(big.Float).SetInt(x).Cmp(big.NewFloat(f))})
		}
	}
	if err := cf.Write(out, "big/cmp_float.jsonl"); err != nil {
		return err
	}

	var tf goldenio.Lines
	for _, x := range roundingCorpus() {
		f, acc := x.Float64()
		tf.Add(map[string]any{"x": x.Text(16), "f": fbits(f), "acc": accInt(acc)})
	}
	return tf.Write(out, "big/to_float.jsonl")
}
