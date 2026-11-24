mod jsrun;
mod tzchild;

use goja::Value;
use goja::date::DateObject;
use jsrun::*;
use tzchild::{Zone, run_in_zone};

#[test]
fn test_date_utc() {
    const SCRIPT: &str = r#"
	assert.sameValue(Date.UTC(1970, 0), 0, '1970, 0');
	assert.sameValue(Date.UTC(2016, 0), 1451606400000, '2016, 0');
	assert.sameValue(Date.UTC(2016, 6), 1467331200000, '2016, 6');

	assert.sameValue(Date.UTC(2016, 6, 1), 1467331200000, '2016, 6, 1');
	assert.sameValue(Date.UTC(2016, 6, 5), 1467676800000, '2016, 6, 5');

	assert.sameValue(Date.UTC(2016, 6, 5, 0), 1467676800000, '2016, 6, 5, 0');
	assert.sameValue(Date.UTC(2016, 6, 5, 15), 1467730800000, '2016, 6, 5, 15');

	assert.sameValue(
  		Date.UTC(2016, 6, 5, 15, 0), 1467730800000, '2016, 6, 5, 15, 0'
	);
	assert.sameValue(
  		Date.UTC(2016, 6, 5, 15, 34), 1467732840000, '2016, 6, 5, 15, 34'
	);

	assert.sameValue(
  		Date.UTC(2016, 6, 5, 15, 34, 0), 1467732840000, '2016, 6, 5, 15, 34, 0'
	);
	assert.sameValue(
  		Date.UTC(2016, 6, 5, 15, 34, 45), 1467732885000, '2016, 6, 5, 15, 34, 45'
	);

	"#;

    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_new_date() {
    const SCRIPT: &str = r#"
	var d1 = new Date("2016-09-01T12:34:56Z");
	d1.getUTCHours() === 12;

	"#;
    test_script(SCRIPT, Value::Bool(true));
}

#[test]
fn test_new_date0() {
    const SCRIPT: &str = r#"
	(new Date(0)).toUTCString();

	"#;
    test_script(SCRIPT, str_value("Thu, 01 Jan 1970 00:00:00 GMT"));
}

#[test]
fn test_set_hour() {
    run_in_zone("test_set_hour", Zone::Load("America/New_York"), || {
        const SCRIPT: &str = r#"
	var d = new Date(2016, 8, 1, 12, 23, 45)
	assert.sameValue(d.getHours(), 12);
	assert.sameValue(d.getUTCHours(), 16);

	d.setHours(13);
	assert.sameValue(d.getHours(), 13);
	assert.sameValue(d.getMinutes(), 23);
	assert.sameValue(d.getSeconds(), 45);

	d.setUTCHours(13);
	assert.sameValue(d.getHours(), 9);
	assert.sameValue(d.getMinutes(), 23);
	assert.sameValue(d.getSeconds(), 45);

	"#;
        test_script_with_test_lib(SCRIPT, Value::Undefined);
    });
}

#[test]
fn test_set_minute() {
    run_in_zone(
        "test_set_minute",
        Zone::Fixed("Asia/Delhi", 5 * 60 * 60 + 30 * 60),
        || {
            const SCRIPT: &str = r#"
	var d = new Date(2016, 8, 1, 12, 23, 45)
	assert.sameValue(d.getHours(), 12);
	assert.sameValue(d.getUTCHours(), 6);
	assert.sameValue(d.getMinutes(), 23);
	assert.sameValue(d.getUTCMinutes(), 53);

	d.setMinutes(55);
	assert.sameValue(d.getMinutes(), 55);
	assert.sameValue(d.getSeconds(), 45);

	d.setUTCMinutes(52);
	assert.sameValue(d.getMinutes(), 22);
	assert.sameValue(d.getHours(), 13);

	"#;
            test_script_with_test_lib(SCRIPT, Value::Undefined);
        },
    );
}

#[test]
fn test_timezone_offset() {
    run_in_zone("test_timezone_offset", Zone::Load("Europe/London"), || {
        const SCRIPT: &str = r#"
	var d = new Date(0);
	d.getTimezoneOffset();
	"#;

        test_script(SCRIPT, int(-60));
    });
}

#[test]
fn test_date_value_of() {
    const SCRIPT: &str = r#"
	var d9 = new Date(1.23e15);
	d9.valueOf();
	"#;

    test_script(SCRIPT, int(1_230_000_000_000_000));
}

#[test]
fn test_date_setters() {
    run_in_zone("test_date_setters", Zone::Load("Europe/London"), || {
        const SCRIPT: &str = r#"
	assert.sameValue((new Date(0)).setMilliseconds(2345), 2345, "setMilliseconds(2345)");
	assert.sameValue(new Date(1000).setMilliseconds(23450000000000), 23450000001000, "setMilliseconds(23450000000000)");
	assert.sameValue((new Date(0)).setUTCMilliseconds(2345), 2345, "setUTCMilliseconds()");
	assert.sameValue((new Date(0)).setSeconds(12), 12000, "setSeconds()");
	assert.sameValue((new Date(0)).setUTCSeconds(12), 12000, "setUTCSeconds()");
	assert.sameValue((new Date(0)).setMinutes(12), 12 * 60 * 1000, "setMinutes()");
	assert.sameValue((new Date(0)).setUTCMinutes(12), 12 * 60 * 1000, "setUTCMinutes()");
	assert.sameValue((new Date("2016-06-01")).setHours(1), 1464739200000, "setHours()");
	assert.sameValue((new Date("2016-06-01")).setUTCHours(1), 1464742800000, "setUTCHours()");
	assert.sameValue((new Date(0)).setDate(2), 86400000, "setDate()");
	assert.sameValue((new Date(0)).setUTCDate(2), 86400000, "setUTCDate()");
	assert.sameValue((new Date(0)).setMonth(2), 5097600000, "setMonth()");
	assert.sameValue((new Date(0)).setUTCMonth(2), 5097600000, "setUTCMonth()");
	assert.sameValue((new Date(0)).setFullYear(1971), 31536000000, "setFullYear()");
	assert.sameValue((new Date(0)).setFullYear(1971, 2, 3), 36806400000, "setFullYear(Y,M,D)");
	assert.sameValue((new Date(0)).setUTCFullYear(1971), 31536000000, "setUTCFullYear()");
	assert.sameValue((new Date(0)).setUTCFullYear(1971, 2, 3), 36806400000, "setUTCFullYear(Y,M,D)");

	var d = new Date();
	d.setTime(1151877845000);
	assert.sameValue(d.getHours(), 23, "d.getHours()");
	"#;

        test_script_with_test_lib(SCRIPT, Value::Undefined);
    });
}

#[test]
fn test_date_parse() {
    run_in_zone("test_date_parse", Zone::Load("America/New_York"), || {
        const SCRIPT: &str = r#"
	var zero = new Date(0);

	assert.sameValue(zero.valueOf(), Date.parse(zero.toString()),
					 "Date.parse(zeroDate.toString())");
	assert.sameValue(zero.valueOf(), Date.parse(zero.toUTCString()),
					 "Date.parse(zeroDate.toUTCString())");
	assert.sameValue(zero.valueOf(), Date.parse(zero.toISOString()),
					 "Date.parse(zeroDate.toISOString())");

	function testParse(str, expected) {
		assert.sameValue(Date.parse(str), expected, str);
	}

	testParse("Mon, 02 Jan 2006 15:04:05 MST",							1136239445000);
	testParse("Tue, 22 Jun 2021 13:54:40 GMT",							1624370080000);
	testParse("Tuesday, 22 Jun 2021 13:54:40 GMT",						1624370080000);
	testParse("Mon, 02 Jan 2006 15:04:05 GMT-07:00 (MST)",				1136239445000);
	testParse("Mon, 02 Jan 2006 15:04:05 -07:00 (MST)",					1136239445000);
	testParse("Monday, 02 Jan 2006 15:04:05 -0700 (MST)",				1136239445000);
	testParse("Mon Jan 02 2006 15:04:05 GMT-0700 (GMT Standard Time)",	1136239445000);
	testParse("Mon Jan 2 15:04:05 MST 2006",							1136239445000);
	testParse("Mon Jan 02 15:04:05 MST 2006",							1136239445000);
	testParse("Mon Jan  2 15:04:05 2006",								1136232245000);
	testParse("Mon Jan 02 15:04:05 -0700 2006",							1136239445000);
	testParse("Mon Jan 02 3:4 PM -0700 2006",							1136239440000);

	testParse("December 04, 1986",	534056400000);
	testParse("Dec 04, 1986",		534056400000);
	testParse("Dec 4, 1986",		534056400000);

	testParse("2006-01-02T15:04:05.000Z",	1136214245000);
	testParse("2006-06-02T15:04:05.000",	1149275045000);
	testParse("2006-01-02T15:04:05",		1136232245000);
	testParse("2006-01-02 15:04:05.123",	1136232245123);
	testParse("2006-01-02",					1136160000000);
	testParse("2006T15:04-0700",			1136153040000);
	testParse("2006T15:04+07:00",			1136102640000);
	testParse("2006T15:04Z",				1136127840000);
	testParse("2019-01-01T12:00:00.52Z",	1546344000520);
	testParse("2019-01T12:00:00.52Z",		1546344000520);
	testParse("+002019-01-01T12:00:00.52Z",	1546344000520);

	var d = new Date("Mon, 02 Jan 2006 15:04:05 MST");

	assert.sameValue(d.getUTCHours(), 22,
					"new Date(\"Mon, 02 Jan 2006 15:04:05 MST\").getUTCHours()");

	assert.sameValue(d.getHours(), 17,
					"new Date(\"Mon, 02 Jan 2006 15:04:05 MST\").getHours()");

	assert.sameValue(Date.parse("Mon, 02 Jan 2006 15:04:05 zzz"), NaN,
					 "Date.parse(\"Mon, 02 Jan 2006 15:04:05 zzz\")");

	assert.sameValue(Date.parse("Mon, 02 Jan 2006 15:04:05 ZZZ"), NaN,
					 "Date.parse(\"Mon, 02 Jan 2006 15:04:05 ZZZ\")");

	var minDateStr = "-271821-04-20T00:00:00.000Z";
	var minDate = new Date(-8640000000000000);

	assert.sameValue(minDate.toISOString(), minDateStr, "minDateStr");
	assert.sameValue(Date.parse(minDateStr), minDate.valueOf(), "parse minDateStr");

	var maxDateStr = "+275760-09-13T00:00:00.000Z";
	var maxDate = new Date(8640000000000000);

	assert.sameValue(maxDate.toISOString(), maxDateStr, "maxDateStr");
	assert.sameValue(Date.parse(maxDateStr), maxDate.valueOf(), "parse maxDateStr");

	var belowRange = "-271821-04-19T23:59:59.999Z";
	var aboveRange = "+275760-09-13T00:00:00.001Z";

	assert.sameValue(Date.parse(belowRange), NaN, "parse below minimum time value");
	assert.sameValue(Date.parse(aboveRange), NaN, "parse above maximum time value");
	"#;

        test_script_with_test_lib(SCRIPT, Value::Undefined);
    });
}

#[test]
fn test_date_max_values() {
    const SCRIPT: &str = r#"
	assert.sameValue((new Date(0)).setUTCMilliseconds(8.64e15), 8.64e15);
	assert.sameValue((new Date(0)).setUTCSeconds(8640000000000), 8.64e15);
	assert.sameValue((new Date(0)).setUTCMilliseconds(-8.64e15), -8.64e15);
	assert.sameValue((new Date(0)).setUTCSeconds(-8640000000000), -8.64e15);
	"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}

#[test]
fn test_date_export() {
    on_big_stack(|| {
        let mut rt = new_runtime();
        let res = match rt.run_string("new Date(1000)") {
            Ok(v) => v,
            Err(e) => panic!("{}", run_error_string(&e)),
        };
        let o = res.as_object().expect("an object");
        let exp = rt
            .ext_dyn::<DateObject>(o)
            .and_then(DateObject::export)
            .unwrap_or_else(|| panic!("Invalid export type: {res:?}"));
        assert_eq!(
            exp.unix_nano() / 1_000_000,
            1000,
            "Invalid exported date: {exp:?}"
        );
        assert!(
            exp.location().is_local(),
            "Invalid timezone: {}",
            exp.location()
        );
    });
}

#[test]
fn test_date_to_json() {
    const SCRIPT: &str = r#"
	Date.prototype.toJSON.call({ toISOString: function () { return 1; } })
	"#;
    test_script(SCRIPT, int(1));
}

#[test]
#[ignore = "requires the Array builtin"]
fn test_date_parse_v8() {
    const SCRIPT: &str = r#"
const assertEquals = assert.sameValue,
	assertTrue = assert;

function testDateParse(string) {
  var d = Date.parse(string);
  assertEquals(946713600000, d, "parse: " + string);
};
function testDateParseLocalTime(string) {
  var d = Date.parse("parse-local-time:" + string);
  assertTrue(!isNaN(d), "parse-local-time: " + string + " is NaN.");
  assertTrue(d > 0, "parse-local-time: " + string + " <= 0.");
};
function testDateParseMisc(array) {
  assertEquals(2, array.length, "array [" + array + "] length != 2.");
  var string = array[0];
  var expected = array[1];
  var d = Date.parse(string);
  assertEquals(expected, d, "parse-misc: " + string);
}
var testCasesUT = [
    'Sat, 01-Jan-2000 08:00:00 UT',
    'Sat, 01 Jan 2000 08:00:00 UT',
    'Jan 01 2000 08:00:00 UT',
    'Jan 01 08:00:00 UT 2000',
    'Saturday, 01-Jan-00 08:00:00 UT',
    '01 Jan 00 08:00 +0000',
    'Mon, 01 Jan 2000 08:00:00 UT',
    'Tue, 01 Jan 2000 08:00:00 UT',
    '[Saturday] Jan 01 08:00:00 UT 2000',
    'Ignore all of this stuff because it is annoying 01 Jan 2000 08:00:00 UT',
    '[Saturday] Jan 01 2000 08:00:00 UT',
    'All of this stuff is really annoying, so it will be ignored Jan 01 2000 08:00:00 UT',
    'Sat, 01-Janisamonth-2000 08:00:00 UT',
    'Sat, 01 Janisamonth 2000 08:00:00 UT',
    'Janisamonth 01 2000 08:00:00 UT',
    'Janisamonth 01 08:00:00 UT 2000',
    'Saturday, 01-Janisamonth-00 08:00:00 UT',
    '01 Janisamonth 00 08:00 +0000',
    'Janisamonthandtherestisignored01 2000 08:00:00 UT',
    'Jan01 2000 08:00:00 UT',
    'Sat, 2000/01/01 08:00:00 UT',
    'Sat, 01/01/2000 08:00:00 UT',
    'Sat, 01/01 2000 08:00:00 UT',
    'Sat, 01,Jan,2000,08:00:00 UT',
    'Sat, 01-Jan-2000 08:00 UT',
    'Sat, 01 Jan 2000 08:00 UT',
    'Jan 01 2000 08:00 UT',
    'Jan 01 08:00 UT 2000',
    'Saturday, 01-Jan-00 08:00 UT',
    '01 Jan 00 08:00 +0000',
    'Sat, 01-Jan-2000 08:00 AM UT',
    'Sat, 01 Jan 2000 08:00 AM UT',
    'Jan 01 2000 08:00 AM UT',
    'Jan 01 08:00 AM UT 2000',
    'Saturday, 01-Jan-00 08:00 AM UT',
    '01 Jan 00 08:00 AM +0000',
    '   Sat,   01-Jan-2000   08:00:00   UT  ',
    '  Sat,   01   Jan   2000   08:00:00   UT  ',
    '  Saturday,   01-Jan-00   08:00:00   UT  ',
    '  01    Jan   00    08:00   +0000   ',
    ' ()(Sat, 01-Jan-2000)  Sat,   01-Jan-2000   08:00:00   UT  ',
    '  Sat()(Sat, 01-Jan-2000)01   Jan   2000   08:00:00   UT  ',
    '  Sat,(02)01   Jan   2000   08:00:00   UT  ',
    '  Sat,  01(02)Jan   2000   08:00:00   UT  ',
    '  Sat,  01  Jan  2000 (2001)08:00:00   UT  ',
    '  Sat,  01  Jan  2000 (01)08:00:00   UT  ',
    '  Sat,  01  Jan  2000 (01:00:00)08:00:00   UT  ',
    '  Sat,  01  Jan  2000  08:00:00 (CDT)UT  ',
    '  Sat,  01  Jan  2000  08:00:00  UT((((CDT))))',
    '  Saturday,   01-Jan-00 ()(((asfd)))(Sat, 01-Jan-2000)08:00:00   UT  ',
    '  01    Jan   00    08:00 ()(((asdf)))(Sat, 01-Jan-2000)+0000   ',
    '  01    Jan   00    08:00   +0000()((asfd)(Sat, 01-Jan-2000)) '];
var testCasesGMT = [
    'Sat, 01-Jan-2000 08:00:00 GMT',
    'Sat, 01-Jan-2000 08:00:00 GMT+0',
    'Sat, 01-Jan-2000 08:00:00 GMT+00',
    'Sat, 01-Jan-2000 08:00:00 GMT+000',
    'Sat, 01-Jan-2000 08:00:00 GMT+0000',
    'Sat, 01-Jan-2000 08:00:00 GMT+00:00',
    'Sat, 01 Jan 2000 08:00:00 GMT',
    'Saturday, 01-Jan-00 08:00:00 GMT',
    '01 Jan 00 08:00 -0000',
    '01 Jan 00 08:00 +0000'];
var testCasesEST = [
    'Sat, 01-Jan-2000 03:00:00 UTC-0500',
    'Sat, 01-Jan-2000 03:00:00 UTC-05:00',
    'Sat, 01-Jan-2000 03:00:00 EST',
    'Sat, 01 Jan 2000 03:00:00 EST',
    'Saturday, 01-Jan-00 03:00:00 EST',
    '01 Jan 00 03:00 -0500'];
var testCasesEDT = [
    'Sat, 01-Jan-2000 04:00:00 EDT',
    'Sat, 01 Jan 2000 04:00:00 EDT',
    'Saturday, 01-Jan-00 04:00:00 EDT',
    '01 Jan 00 04:00 -0400'];
var testCasesCST = [
    'Sat, 01-Jan-2000 02:00:00 CST',
    'Sat, 01 Jan 2000 02:00:00 CST',
    'Saturday, 01-Jan-00 02:00:00 CST',
    '01 Jan 00 02:00 -0600'];
var testCasesCDT = [
    'Sat, 01-Jan-2000 03:00:00 CDT',
    'Sat, 01 Jan 2000 03:00:00 CDT',
    'Saturday, 01-Jan-00 03:00:00 CDT',
    '01 Jan 00 03:00 -0500'];
var testCasesMST = [
    'Sat, 01-Jan-2000 01:00:00 MST',
    'Sat, 01 Jan 2000 01:00:00 MST',
    'Saturday, 01-Jan-00 01:00:00 MST',
    '01 Jan 00 01:00 -0700'];
var testCasesMDT = [
    'Sat, 01-Jan-2000 02:00:00 MDT',
    'Sat, 01 Jan 2000 02:00:00 MDT',
    'Saturday, 01-Jan-00 02:00:00 MDT',
    '01 Jan 00 02:00 -0600'];
var testCasesPST = [
    'Sat, 01-Jan-2000 00:00:00 PST',
    'Sat, 01 Jan 2000 00:00:00 PST',
    'Saturday, 01-Jan-00 00:00:00 PST',
    '01 Jan 00 00:00 -0800',
    'Sat, 01-Jan-2000 PST'];
var testCasesPDT = [
    'Sat, 01-Jan-2000 01:00:00 PDT',
    'Sat, 01 Jan 2000 01:00:00 PDT',
    'Saturday, 01-Jan-00 01:00:00 PDT',
    '01 Jan 00 01:00 -0700'];
var testCasesLocalTime = [
    'Sat, 01-Jan-2000 08:00:00',
    'Sat, 01 Jan 2000 08:00:00',
    'Jan 01 2000 08:00:00',
    'Jan 01 08:00:00 2000',
    'Saturday, 01-Jan-00 08:00:00',
    '01 Jan 00 08:00'];
var testCasesMisc = [
    ['Sat, 01 Jan 0 08:00:00 UT', 946713600000],
    ['Sat, 01 Jan 49 08:00:00 UT', 2493100800000],
    ['Sat, 01 Jan 50 08:00:00 UT', -631123200000],
    ['Sat, 01 Jan 99 08:00:00 UT', 915177600000],
    ['Sat, 01 Jan 100 08:00:00 UT', -59011430400000],
    ['Sat, 01-Jan-2000 08:00 PM UT', 946756800000],
    ['Sat, 01 Jan 2000 08:00 PM UT', 946756800000],
    ['Jan 01 2000 08:00 PM UT', 946756800000],
    ['Jan 01 08:00 PM UT 2000', 946756800000],
    ['Saturday, 01-Jan-00 08:00 PM UT', 946756800000],
    ['01 Jan 00 08:00 PM +0000', 946756800000]];
var testCasesES5Misc = [
    ['2000-01-01T08:00:00.000Z', 946713600000],
    ['2000-01-01T08:00:00Z', 946713600000],
    ['2000-01-01T08:00Z', 946713600000],
    ['2000-01T08:00:00.000Z', 946713600000],
    ['2000T08:00:00.000Z', 946713600000],
    ['2000T08:00Z', 946713600000],
    ['2000-01T00:00:00.000-08:00', 946713600000],
    ['2000-01T08:00:00.001Z', 946713600001],
    ['2000-01T08:00:00.099Z', 946713600099],
    ['2000-01T08:00:00.999Z', 946713600999],
    ['2000-01T00:00:00.001-08:00', 946713600001],
    ['2000-01-01T24:00Z', 946771200000],
    ['2000-01-01T24:00:00Z', 946771200000],
    ['2000-01-01T24:00:00.000Z', 946771200000],
    ['2000-01-01T24:00:00.000Z', 946771200000]];
var testCasesES5MiscNegative = [
    '2000-01-01TZ',
    '2000-01-01T60Z',
    '2000-01-01T60:60Z',
    '2000-01-0108:00Z',
    '2000-01-01T08Z',
    '2000-01-01T24:01',
    '2000-01-01T24:00:01',
    '2000-01-01T24:00:00.001',
    '2000-01-01T24:00:00.999Z'];
var localOffset = new Date('2000-01-01').getTimezoneOffset()*1000*60;
if (this.Intl &&
    ["America/Los_Angeles", "Europe/Berlin", "Europe/Madrid"].indexOf(
        Intl.DateTimeFormat().resolvedOptions().timeZone) != -1) {
  assertTrue(localOffset != 0);
}
var testCasesES2016TZ = [
    ['2000-01-02T00:00', 946771200000 + localOffset],
    ['2000-01-02T00:00:00', 946771200000 + localOffset],
    ['2000-01-02T00:00:00.000', 946771200000 + localOffset],
    ['2000-01-02', 946771200000],
    ['2000-01-02', 946771200000],
    ['2000-01-02', 946771200000],
];
testCasesUT.forEach(testDateParse);
testCasesGMT.forEach(testDateParse);
testCasesEST.forEach(testDateParse);
testCasesEDT.forEach(testDateParse);
testCasesCST.forEach(testDateParse);
testCasesCDT.forEach(testDateParse);
testCasesMST.forEach(testDateParse);
testCasesMDT.forEach(testDateParse);
testCasesPST.forEach(testDateParse);
testCasesPDT.forEach(testDateParse);
testCasesLocalTime.forEach(testDateParseLocalTime);
testCasesMisc.forEach(testDateParseMisc);
testCasesES5Misc.forEach(testDateParseMisc);
testCasesES5MiscNegative.forEach(function (s) {
    assertTrue(isNaN(Date.parse(s)), s + " is not NaN.");
});
testCasesES2016TZ.forEach(testDateParseMisc);
for (var i = 0; i < 24 * 365 * 100; i += 150) {
  var ms = i * (3600 * 1000);
  var s = (new Date(ms)).toString();
  assertEquals(ms, Date.parse(s), "parse own: " + s);
}
var testCasesNegative = [
    'May 25 2008 1:30 (PM)) UTC',
    'May 25 2008 1:30( )AM (PM)',
    'a1',
    'nasfdjklsfjoaifg1',
    'x_2',
    'May 25 2008 AAA (GMT)'];
testCasesNegative.forEach(function (s) {
    assertTrue(isNaN(Date.parse(s)), s + " is not NaN.");
});
`
	testScriptWithTestLib(SCRIPT, _undefined, t)

"#;
    test_script_with_test_lib(SCRIPT, Value::Undefined);
}
