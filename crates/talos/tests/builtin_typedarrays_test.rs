mod jsrun;

use jsrun::*;
use talos::Value;
use talos::typedarrays::{BIG_ENDIAN, LITTLE_ENDIAN};

#[test]
fn test_array_buffer_set_uint32() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let proto = vm.global.array_buffer_prototype;
        let b = vm._new_array_buffer(proto, None);
        let data = vm.array_buffer_shared(b);
        data.borrow_mut().data = vec![0u8; 4];
        data.borrow_mut().set_uint32(0, 0xCAFEBABE, BIG_ENDIAN);

        let i = data.borrow().get_uint32(0, BIG_ENDIAN);
        assert_eq!(i, 0xCAFEBABE, "{i}");
        let i = data.borrow().get_uint32(0, LITTLE_ENDIAN);
        assert_eq!(i, 0xBEBAFECA, "{i}");

        data.borrow_mut().set_uint32(0, 0xBEBAFECA, LITTLE_ENDIAN);
        let i = data.borrow().get_uint32(0, BIG_ENDIAN);
        assert_eq!(i, 0xCAFEBABE, "{i}");
    });
}

#[test]
fn test_array_buffer_set_int32() {
    on_big_stack(|| {
        let mut vm = new_runtime();
        let proto = vm.global.array_buffer_prototype;
        let b = vm._new_array_buffer(proto, None);
        let data = vm.array_buffer_shared(b);
        data.borrow_mut().data = vec![0u8; 4];
        data.borrow_mut().set_int32(0, -42, LITTLE_ENDIAN);
        let v = data.borrow().get_int32(0, LITTLE_ENDIAN);
        assert_eq!(v, -42, "{v}");

        data.borrow_mut().set_int32(0, -42, BIG_ENDIAN);
        let v = data.borrow().get_int32(0, BIG_ENDIAN);
        assert_eq!(v, -42, "{v}");
    });
}

#[test]
fn test_new_uint8_array() {
    const SCRIPT: &str = r#"
	var a = new Uint8Array(1);
	a[0] = 42;
	a.byteLength === 1 && a.length === 1 && a[0] === 42;
	"#;

    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_new_uint16_array() {
    const SCRIPT: &str = r#"
	var a = new Uint16Array(1);
	a[0] = 42;
	a.byteLength === 2 && a.length === 1 && a[0] === 42;
	"#;

    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_typed_arrays_species_constructor() {
    const SCRIPT: &str = r#"
    'use strict';
    function MyArray() {
        var NewTarget = this.__proto__.constructor;
        return Reflect.construct(Uint16Array, arguments, NewTarget);
    }
    MyArray.prototype = Object.create(Uint16Array.prototype, {
        constructor: {
            value: MyArray,
            writable: true,
            configurable: true
        }
    });
    var a = new MyArray(1);
    Object.defineProperty(MyArray, Symbol.species, {value: Uint8Array, configurable: true});
    a[0] = 32767;
    var b = a.filter(function() {
        return true;
    });
	if (a[0] !== 32767) {
		throw new Error("a[0]=" + a[0]);
	}
	if (!(b instanceof Uint8Array)) {
		throw new Error("b instanceof Uint8Array");
	}
	if (b[0] != 255) {
		throw new Error("b[0]=" + b[0]);
	}
	"#;

    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_from_array_buffer() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(2);
	var a16 = new Uint16Array(buf);
	if (!(a16 instanceof Uint16Array)) {
		throw new Error("a16 is not an instance");
	}
	if (a16.buffer !== buf) {
		throw new Error("a16.buffer !== buf");
	}
	if (a16.length !== 1) {
		throw new Error("a16.length=" + a16.length);
	}
	var a8 = new Uint8Array(buf);
	a8.fill(0xAA);
	if (a16[0] !== 0xAAAA) {
		throw new Error("a16[0]=" + a16[0]);
	}
	"#;

    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_overlap_dif_size() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(4);
	var src = new Uint8Array(buf, 1, 2);
	src[0] = 1;
	src[1] = 2;
	var dst = new Uint16Array(buf);
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_overlap_dif_size2() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(4);
	var src = new Uint8Array(buf, 0, 2);
	src[0] = 1;
	src[1] = 2;
	var dst = new Uint16Array(buf);
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_overlap_dif_size3() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(8);
	var src = new Uint8Array(buf, 2, 4);
	src[0] = 1;
	src[1] = 2;
	src[2] = 3;
	src[3] = 4;
	var dst = new Uint16Array(buf);
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2 || dst[2] !== 3 || dst[3] !== 4) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_overlap_dif_size4() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(10);
	var dst = new Uint8Array(buf, 2, 5);
	var src = new Uint16Array(buf);
	src[0] = 1;
	src[1] = 2;
	src[2] = 3;
	src[3] = 4;
	src[4] = 5;
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2 || dst[2] !== 3 || dst[3] !== 4 || dst[4] !== 5) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_no_overlap_dif_size_forward() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(10);
	var dst = new Uint8Array(buf, 7, 2);
	var src = new Uint16Array(buf, 0, 2);
	src[0] = 1;
	src[1] = 2;
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2 || src[0] !== 1 || src[1] !== 2) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_no_overlap_dif_size_backward() {
    const SCRIPT: &str = r#"
	var buf = new ArrayBuffer(10);
	var dst = new Uint8Array(buf, 0, 2);
	var src = new Uint16Array(buf, 6, 2);
	src[0] = 1;
	src[1] = 2;
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2 || src[0] !== 1 || src[1] !== 2) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_set_no_overlap_dif_size_dif_buffers() {
    const SCRIPT: &str = r#"
	var dstBuf = new ArrayBuffer(1024);
	var dst = new Uint8Array(dstBuf, 0, 2);
	var src = new Uint16Array(2);
	src[0] = 1;
	src[1] = 2;
	dst.set(src);
	if (dst[0] !== 1 || dst[1] !== 2 || src[0] !== 1 || src[1] !== 2) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_slice_same_type() {
    const SCRIPT: &str = r#"
	var src = Uint8Array.of(1,2,3,4);
	var dst = src.slice(1, 3);
	if (dst.length !== 2 || dst[0] !== 2 || dst[1] !== 3) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_slice_dif_type() {
    const SCRIPT: &str = r#"
	var src = Uint8Array.of(1,2,3,4);
	Object.defineProperty(Uint8Array, Symbol.species, {value: Uint16Array, configurable: true});
	var dst = src.slice(1, 3);
	if (!(dst instanceof Uint16Array)) {
		throw new Error("wrong dst type: " + dst);
	}
	if (dst.length !== 2 || dst[0] !== 2 || dst[1] !== 3) {
		throw new Error("dst: " + dst.join(","));
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_sort_comparator_return_value_floats() {
    const SCRIPT: &str = r#"
	var a = Float64Array.of(
		5.97,
		9.91,
		4.13,
		9.28,
		3.29
	);
	a.sort( function(a, b) { return a - b; } );
	for (var i = 1; i < a.length; i++) {
		if (a[i] < a[i-1]) {
			throw new Error("Array is not sorted: " + a);
		}
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_sort_comparator_return_value_neg_zero() {
    const SCRIPT: &str = r#"
	var a = new Uint8Array([2, 1]);
	a.sort( function(a, b) { return a > b ? 0 : -0; } );
	for (var i = 1; i < a.length; i++) {
		if (a[i] < a[i-1]) {
			throw new Error("Array is not sorted: " + a);
		}
	}
	"#;
    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_int32_array_negative_index() {
    const SCRIPT: &str = r#"
	new Int32Array()[-1] === undefined;
	"#;

    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_typed_array_delete_unconfigurable() {
    const SCRIPT: &str = r#"
	try {
		(function() {
			'use strict';
			delete Uint8Array.prototype.BYTES_PER_ELEMENT;
		})();
	} catch(e) {
		if (!(e instanceof TypeError)) {
			throw e;
		}
		if (!e.message.startsWith("Cannot delete property")) {
			throw e;
		}
	}
	"#;

    test_script(SCRIPT, Value::Undefined);
}

#[test]
fn test_typed_array_export_to_bytes() {
    const SCRIPT: &str = r#"
	const arr = new Uint16Array(10);
	arr[2] = 0xFFFF; // make sure it's not affected by the platform endianness
	arr.subarray(2, 4);
	"#;
    on_big_stack(|| {
        let mut vm = new_runtime();
        let v = match vm.run_string(SCRIPT) {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let b = vm.export_to_bytes(&v).expect("export").expect("bytes");
        assert_eq!(b, vec![0xFF, 0xFF, 0, 0], "unexpected value {b:?}");
    });
}

#[test]
fn test_data_view_export_to_bytes() {
    const SCRIPT: &str = r#"
	const arr = new Uint16Array(10);
	arr[2] = 0xFFFF;
	new DataView(arr.buffer, 4, 4);
	"#;
    on_big_stack(|| {
        let mut vm = new_runtime();
        let v = match vm.run_string(SCRIPT) {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let b = vm.export_to_bytes(&v).expect("export").expect("bytes");
        assert_eq!(b, vec![0xFF, 0xFF, 0, 0], "unexpected value {b:?}");
    });
}

#[test]
fn test_typed_array_subarray_set() {
    const SCRIPT: &str = r#"
	const u = new Uint8Array(4)
	const s = u.subarray(1, 4);
	s.set([1,2,3], 0);
	if (s.length !== 3) {
		throw new Error("s.length=" + s.length + ", expected 3");
	}
	if (s[0] !== 1 || s[1] !== 2 || s[2] !== 3) {
		throw new Error("s=[" + s.join(",") + "], expected [1,2,3]");
	}
	"#;

    test_script(SCRIPT, Value::Undefined);
}
