mod jsrun;

use jsrun::*;
use talos::host::{DocValue, HandleKind, HostFn};
use talos::typedarrays::{LITTLE_ENDIAN, NATIVE_ENDIAN};
use talos::{Runtime, Value};

fn run(vm: &mut Runtime, src: &str) -> Value {
    match vm.run_string(src) {
        Ok(v) => v,
        Err(e) => panic!("{}", run_error_string(&e)),
    }
}

fn set_detach_buffer(vm: &mut Runtime) {
    let f = vm.new_host_func(HostFn::new("$DETACHBUFFER", |rt, call| {
        if let Value::Object(o) = call.arg(0) {
            rt.array_buffer_detach(*o);
        }
        Ok(Value::Undefined)
    }));
    vm.set_var("$DETACHBUFFER", Value::Object(f)).expect("set");
}

#[test]
fn test_uint16_array_object() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let proto = vm.global.array_buffer_prototype;
        let buf = vm._new_array_buffer(proto, None);
        let data = vm.array_buffer_shared(buf);
        data.borrow_mut().data = vec![0u8; 16];
        if NATIVE_ENDIAN == LITTLE_ENDIAN {
            data.borrow_mut().data[2] = 0xFE;
            data.borrow_mut().data[3] = 0xCA;
        } else {
            data.borrow_mut().data[2] = 0xCA;
            data.borrow_mut().data[3] = 0xFE;
        }
        let a = vm.new_uint16_array_object(buf, 1, 1, None);
        let v = a.get_idx(&mut vm, 0, None).expect("get");
        assert!(matches!(v, Some(Value::Int(0xCAFE))), "v: {v:?}");
    });
}

#[test]
fn test_array_buffer_go_wrapper() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let data = vec![0xAAu8, 0xBB];
        let buf = vm.new_array_buffer(data);
        vm.set_var("buf", Value::Object(buf)).expect("set");
        run(
            &mut vm,
            r#"
	var a = new Uint8Array(buf);
	if (a.length !== 2 || a[0] !== 0xAA || a[1] !== 0xBB) {
		throw new Error(a);
	}
	"#,
        );
        let ret = run(
            &mut vm,
            r#"
	var b = Uint8Array.of(0xCC, 0xDD);
	b.buffer;
	"#,
        );
        let buf1 = match vm.export(&ret).expect("export") {
            DocValue::Object(h) if h.kind == HandleKind::ArrayBuffer => h.obj,
            _ => panic!("not an ArrayBuffer"),
        };
        let data1 = vm.array_buffer_bytes(buf1).expect("bytes");
        assert!(
            data1.len() == 2 && data1[0] == 0xCC && data1[1] == 0xDD,
            "{data1:?}"
        );
        assert!(
            !vm.array_buffer_detached(buf1),
            "buf1.Detached() returned true"
        );
        assert!(vm.array_buffer_detach(buf1), "buf1.Detach() returned false");
        assert!(
            vm.array_buffer_detached(buf1),
            "buf1.Detached() returned false"
        );
        run(
            &mut vm,
            r#"
	if (b[0] !== undefined) {
		throw new Error("b[0] !== undefined");
	}
	"#,
        );
    });
}

#[test]
fn test_typed_array_idx() {
    const SCRIPT: &str = r#"
	var a = new Uint8Array(1);

	// 32-bit integer overflow, should not panic on 32-bit architectures
	if (a[4294967297] !== undefined) {
		throw new Error("4294967297");
	}

	// Canonical non-integer
	a["Infinity"] = 8;
	if (a["Infinity"] !== undefined) {
		throw new Error("Infinity");
	}
	a["NaN"] = 1;
	if (a["NaN"] !== undefined) {
		throw new Error("NaN");
	}

	// Non-canonical integer
	a["00"] = "00";
	if (a["00"] !== "00") {
		throw new Error("00");
	}

	// Non-canonical non-integer
	a["1e-3"] = "1e-3";
	if (a["1e-3"] !== "1e-3") {
		throw new Error("1e-3");
	}
	if (a["0.001"] !== undefined) {
		throw new Error("0.001");
	}

	// Negative zero
	a["-0"] = 88;
	if (a["-0"] !== undefined) {
		throw new Error("-0");
	}

	if (a[0] !== 0) {
		throw new Error("0");
	}

	a["9007199254740992"] = 1;
	if (a["9007199254740992"] !== undefined) {
		throw new Error("9007199254740992");
	}
	a["-9007199254740992"] = 1;
	if (a["-9007199254740992"] !== undefined) {
		throw new Error("-9007199254740992");
	}

	// Safe integer overflow, not canonical (Number("9007199254740993") === 9007199254740992)
	a["9007199254740993"] = 1;
	if (a["9007199254740993"] !== 1) {
		throw new Error("9007199254740993");
	}
	a["-9007199254740993"] = 1;
	if (a["-9007199254740993"] !== 1) {
		throw new Error("-9007199254740993");
	}

	// Safe integer overflow, canonical Number("9007199254740994") == 9007199254740994
	a["9007199254740994"] = 1;
	if (a["9007199254740994"] !== undefined) {
		throw new Error("9007199254740994");
	}
	a["-9007199254740994"] = 1;
	if (a["-9007199254740994"] !== undefined) {
		throw new Error("-9007199254740994");
	}
	"#;

    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_detached_buffer() {
    const SCRIPT: &str = r#"
	let sample = new Uint8Array([42]);
	$DETACHBUFFER(sample.buffer);
	sample[0] = 1;

	assert.sameValue(sample[0], undefined, 'sample[0] = 1 is undefined');
	sample['1.1'] = 1;
	assert.sameValue(sample['1.1'], undefined, 'sample[\'1.1\'] = 1 is undefined');
	sample['-0'] = 1;
	assert.sameValue(sample['-0'], undefined, 'sample[\'-0\'] = 1 is undefined');
	sample['-1'] = 1;
	assert.sameValue(sample['-1'], undefined, 'sample[\'-1\'] = 1 is undefined');
	sample['1'] = 1;
	assert.sameValue(sample['1'], undefined, 'sample[\'1\'] = 1 is undefined');
	sample['2'] = 1;
	assert.sameValue(sample['2'], undefined, 'sample[\'2\'] = 1 is undefined');
	"#;
    on_big_stack(|| {
        let mut vm = new_runtime();
        set_detach_buffer(&mut vm);
        rt_test_script_with_test_lib(&mut vm, SCRIPT, &Value::Undefined);
    });
}

#[test]
fn test_typed_array_define_prop_detached_buffer() {
    const SCRIPT: &str = r#"
	var desc = {
	  value: 0,
	  configurable: false,
	  enumerable: true,
	  writable: true
	};

	var obj = {
	  valueOf: function() {
		throw new Error("valueOf() was called");
	  }
	};
	let sample = new Uint8Array(42);
	$DETACHBUFFER(sample.buffer);

	assert.sameValue(
	Reflect.defineProperty(sample, "0", desc),
	false,
	'Reflect.defineProperty(sample, "0", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "-1", desc),
	false,
	'Reflect.defineProperty(sample, "-1", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "1.1", desc),
	false,
	'Reflect.defineProperty(sample, "1.1", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "-0", desc),
	false,
	'Reflect.defineProperty(sample, "-0", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "2", {
	  configurable: true,
	  enumerable: true,
	  writable: true,
	  value: obj
	}),
	false,
	'Reflect.defineProperty(sample, "2", {configurable: true, enumerable: true, writable: true, value: obj}) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "3", {
	  configurable: false,
	  enumerable: false,
	  writable: true,
	  value: obj
	}),
	false,
	'Reflect.defineProperty(sample, "3", {configurable: false, enumerable: false, writable: true, value: obj}) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "4", {
	  writable: false,
	  configurable: false,
	  enumerable: true,
	  value: obj
	}),
	false,
	'Reflect.defineProperty("new TA(42)", "4", {writable: false, configurable: false, enumerable: true, value: obj}) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "42", desc),
	false,
	'Reflect.defineProperty(sample, "42", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "43", desc),
	false,
	'Reflect.defineProperty(sample, "43", {value: 0, configurable: false, enumerable: true, writable: true} ) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "5", {
	  get: function() {}
	}),
	false,
	'Reflect.defineProperty(sample, "5", {get: function() {}}) must return false'
	);

	assert.sameValue(
	Reflect.defineProperty(sample, "6", {
	  configurable: false,
	  enumerable: true,
	  writable: true
	}),
	false,
	'Reflect.defineProperty(sample, "6", {configurable: false, enumerable: true, writable: true}) must return false'
	);
	"#;
    on_big_stack(|| {
        let mut vm = new_runtime();
        set_detach_buffer(&mut vm);
        rt_test_script_with_test_lib(&mut vm, SCRIPT, &Value::Undefined);
    });
}

#[test]
fn test_typed_array_define_property() {
    const SCRIPT: &str = r#"
	var a = new Uint8Array(1);

	assert.throws(TypeError, function() {
		Object.defineProperty(a, "1", {value: 1});
	});
	assert.sameValue(Reflect.defineProperty(a, "1", {value: 1}), false, "1");

	assert.throws(TypeError, function() {
		Object.defineProperty(a, "Infinity", {value: 8});
	});
	assert.sameValue(Reflect.defineProperty(a, "Infinity", {value: 8}), false, "Infinity");

	Object.defineProperty(a, "test", {value: "passed"});
	assert.sameValue(a.test, "passed", "string property");

	assert.throws(TypeError, function() {
		Object.defineProperty(a, "0", {value: 1, writable: false});
	}, "define non-writable");

	assert.throws(TypeError, function() {
		Object.defineProperty(a, "0", {get() { return 1; }});
	}, "define accessor");

	var sample = new Uint8Array([42, 42]);

	assert.sameValue(
	Reflect.defineProperty(sample, "0", {
	  value: 8,
	  configurable: true,
	  enumerable: true,
	  writable: true
	}),
	true
	);

	assert.sameValue(sample[0], 8, "property value was set");
	let descriptor0 = Object.getOwnPropertyDescriptor(sample, "0");
	assert.sameValue(descriptor0.value, 8);
	assert.sameValue(descriptor0.configurable, true, "configurable");
	assert.sameValue(descriptor0.enumerable, true);
	assert.sameValue(descriptor0.writable, true);
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_get_invalid_index() {
    const SCRIPT: &str = r#"
	var TypedArray = Object.getPrototypeOf(Int8Array);
	var proto = TypedArray.prototype;
	Object.defineProperty(proto, "1", {
		get: function() {
			throw new Error("OrdinaryGet was called!");
		}
	});
	var a = new Uint8Array(1);
	assert.sameValue(a[1], undefined);
	assert.sameValue(a["1"], undefined);
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_export_array_buffer_to_bytes() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let bb = b"test".to_vec();
        let ab = vm.new_array_buffer(bb.clone());
        let b = vm
            .export_to_bytes(&Value::Object(ab))
            .expect("export")
            .expect("bytes");
        assert_eq!(b, bb, "Not equal");

        let r = vm.export_to_bytes(&Value::Int(123)).expect("export");
        assert!(r.is_none(), "expected error");
    });
}

fn export_of(vm: &mut Runtime, src: &str) -> DocValue {
    let v = run(vm, src);
    vm.export(&v).expect("export")
}

fn check_still_works(vm: &mut Runtime) {
    run(
        vm,
        r#"{
		let a = new Uint8Array([1, 2]);
		if (a[0] !== 1 || a[1] !== 2) {
			throw new Error(a);
		}
		}"#,
    );
}

#[test]
fn test_typed_array_export() {
    on_big_stack(|| {
        let mut vm = new_runtime();

        match &export_of(&mut vm, "new Uint8Array([1, 2])") {
            DocValue::Bytes(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == 2, "uint8 {a:?}");
            }
            _ => panic!("uint8: Wrong export type"),
        }
        check_still_works(&mut vm);

        match &export_of(
            &mut vm,
            r#"{
			const buf = new Uint8Array([1, 2]).buffer;
			new Uint8Array(buf, 1, 1);
		}"#,
        ) {
            DocValue::Bytes(a) => {
                let a = a.to_vec();
                assert!(a.len() == 1 && a[0] == 2, "uint8-slice {a:?}");
            }
            _ => panic!("uint8-slice: Wrong export type"),
        }
        check_still_works(&mut vm);

        match &export_of(&mut vm, "new Int8Array([1, -2])") {
            DocValue::Int8s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == -2, "int8 {a:?}");
            }
            _ => panic!("int8: Wrong export type"),
        }

        match &export_of(&mut vm, "new Uint16Array([1, 63000])") {
            DocValue::Uint16s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == 63000, "uint16 {a:?}");
            }
            _ => panic!("uint16: Wrong export type"),
        }

        match &export_of(&mut vm, "new Int16Array([1, -31000])") {
            DocValue::Int16s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == -31000, "int16 {a:?}");
            }
            _ => panic!("int16: Wrong export type"),
        }

        match &export_of(&mut vm, "new Uint32Array([1, 123456])") {
            DocValue::Uint32s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == 123456, "uint32 {a:?}");
            }
            _ => panic!("uint32: Wrong export type"),
        }

        match &export_of(&mut vm, "new Int32Array([1, -123456])") {
            DocValue::Int32s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == -123456, "int32 {a:?}");
            }
            _ => panic!("int32: Wrong export type"),
        }

        match &export_of(&mut vm, "new Float32Array([1, -1.23456])") {
            DocValue::Float32s(a) => {
                let a = a.to_vec();
                assert!(
                    a.len() == 2 && a[0] == 1.0 && a[1] == -1.23456f32,
                    "float32 {a:?}"
                );
            }
            _ => panic!("float32: Wrong export type"),
        }

        match &export_of(&mut vm, "new Float64Array([1, -1.23456789])") {
            DocValue::Float64s(a) => {
                let a = a.to_vec();
                assert!(
                    a.len() == 2 && a[0] == 1.0 && a[1] == -1.23456789,
                    "float64 {a:?}"
                );
            }
            _ => panic!("float64: Wrong export type"),
        }

        match &export_of(&mut vm, "new BigInt64Array([18446744073709551617n, 2n])") {
            DocValue::Int64s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == 2, "bigint64 {a:?}");
            }
            _ => panic!("bigint64: Wrong export type"),
        }

        match &export_of(&mut vm, "new BigUint64Array([18446744073709551617n, 2n])") {
            DocValue::Uint64s(a) => {
                let a = a.to_vec();
                assert!(a.len() == 2 && a[0] == 1 && a[1] == 2, "biguint64 {a:?}");
            }
            _ => panic!("biguint64: Wrong export type"),
        }
    });
}

#[test]
fn test_typed_array_export_shares_the_buffer() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let v = run(
            &mut vm,
            "var a = new Float64Array([1.5, 2.5]); a.subarray(1)",
        );
        let dv = vm.export(&v).expect("export");
        let DocValue::Float64s(s) = &dv else {
            panic!("Wrong export type");
        };
        assert_eq!(s.to_vec(), vec![2.5]);
        run(&mut vm, "a[1] = 7");
        assert_eq!(s.to_vec(), vec![7.0]);
        let w = vm
            .to_value(&DocValue::Float64s(s.clone()))
            .expect("to value");
        vm.set_var("w", w).expect("set");
        run(&mut vm, "w[0] = 9");
        let r = run(&mut vm, "a[1]");
        assert!(matches!(r, Value::Int(9)), "{r:?}");
        assert_eq!(s.to_vec(), vec![9.0]);
    });
}

#[test]
fn test_typed_array_out_of_range_floats_follow_the_architecture() {
    const SCRIPT: &str = r#"
	var a = new Uint8Array(1); a[0] = 1e20;
	var b = new Int32Array(1); b[0] = 1e20;
	var c = new Uint16Array(1); c[0] = -1e20;
	var d = new Int8Array(1); d.fill(1e19);
	var v = new DataView(new ArrayBuffer(4)); v.setInt32(0, 1e20);
	[a[0], b[0], c[0], d[0], v.getInt32(0)].join();
	"#;
    let want = if cfg!(target_arch = "x86_64") {
        "0,0,0,0,0"
    } else {
        "255,-1,0,-1,-1"
    };
    on_big_stack(move || {
        let mut vm = new_runtime();
        let v = run(&mut vm, SCRIPT);
        let s = v.to_string(&mut vm).expect("string").to_rust_string();
        assert_eq!(s, want);
    });
}

#[test]
fn test_typed_array_export_of_unallocated_buffer_is_nil() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        for (src, want) in [
            ("new Uint8Array(0)", "null"),
            ("new Float64Array([])", "null"),
            ("new Uint8Array(new ArrayBuffer(0))", "\"\""),
            ("new Int16Array(new ArrayBuffer(0))", "[]"),
            ("new Uint8Array(new Uint8Array(0))", "\"\""),
            ("new Uint8Array([1, 2]).subarray(1, 1)", "\"\""),
            ("new Uint8Array(new ArrayBuffer())", "null"),
            ("new Float32Array(new Float32Array([1.5]).buffer, 4)", "[]"),
            (
                "new Uint8Array([1,2]).filter(function(){return false})",
                "\"\"",
            ),
            ("new Uint8Array([1, 2]).buffer", "{}"),
            ("new DataView(new ArrayBuffer(2))", "{}"),
            (
                "(function(){ var a = new Uint8Array([1, 2]); var o = {t: a}; a[0] = 9; return o; })()",
                "{\"t\":\"CQI=\"}",
            ),
        ] {
            let dv = export_of(&mut vm, src);
            let b = talos::host::json::marshal(&dv).expect("marshal");
            assert_eq!(String::from_utf8(b).expect("utf8"), want, "{src}");
        }
    });
}
