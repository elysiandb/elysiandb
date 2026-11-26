use talos::deps::regexp2::{
    COMPILED, EXPLICIT_CAPTURE, IGNORE_CASE, MULTILINE, RIGHT_TO_LEFT, RegexOptions, Regexp,
};

#[derive(Default)]
struct Trials {
    failures: Vec<String>,
}

impl Trials {
    fn run_regex_trial<P: AsRef<[u8]> + ?Sized, I: AsRef<[u8]> + ?Sized>(
        &mut self,
        pattern: &P,
        options: RegexOptions,
        input: &I,
        expected: &str,
    ) {
        let pattern_text = String::from_utf8_lossy(pattern.as_ref()).into_owned();
        let input_text = String::from_utf8_lossy(input.as_ref()).into_owned();
        let re = match Regexp::compile(pattern, options) {
            Ok(re) => re,
            Err(err) => {
                if expected != "Error." {
                    self.failures.push(format!(
                        "Compiling pattern '{pattern_text}' with options '{options}' -- expected '{expected}' got '{err}'"
                    ));
                }
                return;
            }
        };
        let group_nums = re.get_group_numbers();
        let result = match re.find_string_match(input) {
            Err(err) => format!("Error. {err}"),
            Ok(Some(m)) => {
                let mut result = String::from("Pass.");
                for (i, group) in m.groups().iter().enumerate() {
                    let gid = group_nums[i];
                    result.push_str(&format!(" Group[{gid}]="));
                    for cp in &group.captures {
                        result.push_str(&format!("({},{})", cp.index, cp.length));
                    }
                }
                result
            }
            Ok(None) => "Fail.".to_string(),
        };
        if expected != result {
            self.failures.push(format!(
                "Matching input '{input_text}' against pattern '{pattern_text}' with options '{options}' -- expected '{expected}' got '{result}'"
            ));
        }
    }
}

#[test]
fn test_mono_basics() {
    let mut t = Trials::default();
    t.run_regex_trial("abc", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc", 0, "xbc", "Fail.");
    t.run_regex_trial("abc", 0, "axc", "Fail.");
    t.run_regex_trial("abc", 0, "abx", "Fail.");
    t.run_regex_trial("abc", 0, "xabcy", "Pass. Group[0]=(1,3)");
    t.run_regex_trial("abc", 0, "ababc", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("ab*c", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", 0, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab*bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial(".{1}", 0, "abbbbc", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(".{3,4}", 0, "abbbbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab{0,}bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab+bc", 0, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab+bc", 0, "abc", "Fail.");
    t.run_regex_trial("ab+bc", 0, "abq", "Fail.");
    t.run_regex_trial("ab{1,}bc", 0, "abq", "Fail.");
    t.run_regex_trial("ab+bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,}bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,3}bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{3,4}bc", 0, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{4,5}bc", 0, "abbbbc", "Fail.");
    t.run_regex_trial("ab?bc", 0, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab?bc", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}bc", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab?bc", 0, "abbbbc", "Fail.");
    t.run_regex_trial("ab?c", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}c", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", 0, "abcc", "Fail.");
    t.run_regex_trial("^abc", 0, "abcc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", 0, "aabc", "Fail.");
    t.run_regex_trial("abc$", 0, "aabc", "Pass. Group[0]=(1,3)");
    t.run_regex_trial("abc$", 0, "aabcd", "Fail.");
    t.run_regex_trial("^", 0, "abc", "Pass. Group[0]=(0,0)");
    t.run_regex_trial("$", 0, "abc", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("a.c", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.c", 0, "axc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.*c", 0, "axyzc", "Pass. Group[0]=(0,5)");
    t.run_regex_trial("a.*c", 0, "axyzd", "Fail.");
    t.run_regex_trial("a[bc]d", 0, "abc", "Fail.");
    t.run_regex_trial("a[bc]d", 0, "abd", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]e", 0, "abd", "Fail.");
    t.run_regex_trial("a[b-d]e", 0, "ace", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]", 0, "aac", "Pass. Group[0]=(1,2)");
    t.run_regex_trial("a[-b]", 0, "a-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-]", 0, "a-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-a]", 0, "-", "Error.");
    t.run_regex_trial("a[]b", 0, "-", "Error.");
    t.run_regex_trial("a[", 0, "-", "Error.");
    t.run_regex_trial("a]", 0, "a]", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[]]b", 0, "a]b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", 0, "aed", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", 0, "abd", "Fail.");
    t.run_regex_trial("a[^-b]c", 0, "adc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^-b]c", 0, "a-c", "Fail.");
    t.run_regex_trial("a[^]b]c", 0, "a]c", "Fail.");
    t.run_regex_trial("a[^]b]c", 0, "adc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("\\ba\\b", 0, "a-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\ba\\b", 0, "-a", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\ba\\b", 0, "-a-", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\by\\b", 0, "xy", "Fail.");
    t.run_regex_trial("\\by\\b", 0, "yz", "Fail.");
    t.run_regex_trial("\\by\\b", 0, "xyz", "Fail.");
    t.run_regex_trial("\\Ba\\B", 0, "a-", "Fail.");
    t.run_regex_trial("\\Ba\\B", 0, "-a", "Fail.");
    t.run_regex_trial("\\Ba\\B", 0, "-a-", "Fail.");
    t.run_regex_trial("\\By\\b", 0, "xy", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\by\\B", 0, "yz", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\By\\B", 0, "xyz", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\w", 0, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\w", 0, "-", "Fail.");
    t.run_regex_trial("\\W", 0, "a", "Fail.");
    t.run_regex_trial("\\W", 0, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a\\sb", 0, "a b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a\\sb", 0, "a-b", "Fail.");
    t.run_regex_trial("a\\Sb", 0, "a b", "Fail.");
    t.run_regex_trial("a\\Sb", 0, "a-b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("\\d", 0, "1", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\d", 0, "-", "Fail.");
    t.run_regex_trial("\\D", 0, "1", "Fail.");
    t.run_regex_trial("\\D", 0, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\w]", 0, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\w]", 0, "-", "Fail.");
    t.run_regex_trial("[\\W]", 0, "a", "Fail.");
    t.run_regex_trial("[\\W]", 0, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a[\\s]b", 0, "a b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[\\s]b", 0, "a-b", "Fail.");
    t.run_regex_trial("a[\\S]b", 0, "a b", "Fail.");
    t.run_regex_trial("a[\\S]b", 0, "a-b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("[\\d]", 0, "1", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\d]", 0, "-", "Fail.");
    t.run_regex_trial("[\\D]", 0, "1", "Fail.");
    t.run_regex_trial("[\\D]", 0, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("ab|cd", 0, "abc", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("ab|cd", 0, "abcd", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("()ef", 0, "def", "Pass. Group[0]=(1,2) Group[1]=(1,0)");
    t.run_regex_trial("*a", 0, "-", "Error.");
    t.run_regex_trial("(*)b", 0, "-", "Error.");
    t.run_regex_trial("$b", 0, "b", "Fail.");
    t.run_regex_trial("a\\", 0, "-", "Error.");
    t.run_regex_trial("a\\(b", 0, "a(b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a\\(*b", 0, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a\\(*b", 0, "a((b", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("a\\\\b", 0, "a\\b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc)", 0, "-", "Error.");
    t.run_regex_trial("(abc", 0, "-", "Error.");
    t.run_regex_trial(
        "((a))",
        0,
        "abc",
        "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1)",
    );
    t.run_regex_trial(
        "(a)b(c)",
        0,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(2,1)",
    );
    t.run_regex_trial("a+b+c", 0, "aabbabc", "Pass. Group[0]=(4,3)");
    t.run_regex_trial("a{1,}b{1,}c", 0, "aabbabc", "Pass. Group[0]=(4,3)");
    t.run_regex_trial("a**", 0, "-", "Error.");
    t.run_regex_trial("a.+?c", 0, "abcabc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial(
        "(a+|b)*",
        0,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,}",
        0,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b)+",
        0,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b){1,}",
        0,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial("(a+|b)?", 0, "ab", "Pass. Group[0]=(0,1) Group[1]=(0,1)");
    t.run_regex_trial(
        "(a+|b){0,1}",
        0,
        "ab",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial(")(", 0, "-", "Error.");
    t.run_regex_trial("[^ab]*", 0, "cde", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc", 0, "", "Fail.");
    t.run_regex_trial("a*", 0, "", "Pass. Group[0]=(0,0)");
    t.run_regex_trial(
        "([abc])*d",
        0,
        "abbbcd",
        "Pass. Group[0]=(0,6) Group[1]=(0,1)(1,1)(2,1)(3,1)(4,1)",
    );
    t.run_regex_trial(
        "([abc])*bcd",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(0,1)",
    );
    t.run_regex_trial("a|b|c|d|e", 0, "e", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "(a|b|c|d|e)f",
        0,
        "ef",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("abcd*efg", 0, "abcdefg", "Pass. Group[0]=(0,7)");
    t.run_regex_trial("ab*", 0, "xabyabbbz", "Pass. Group[0]=(1,2)");
    t.run_regex_trial("ab*", 0, "xayabbbz", "Pass. Group[0]=(1,1)");
    t.run_regex_trial(
        "(ab|cd)e",
        0,
        "abcde",
        "Pass. Group[0]=(2,3) Group[1]=(2,2)",
    );
    t.run_regex_trial("[abhgefdc]ij", 0, "hij", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^(ab|cd)e", 0, "abcde", "Fail.");
    t.run_regex_trial(
        "(abc|)ef",
        0,
        "abcdef",
        "Pass. Group[0]=(4,2) Group[1]=(4,0)",
    );
    t.run_regex_trial("(a|b)c*d", 0, "abcd", "Pass. Group[0]=(1,3) Group[1]=(1,1)");
    t.run_regex_trial(
        "(ab|ab*)bc",
        0,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "a([bc]*)c*",
        0,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(1,2)",
    );
    t.run_regex_trial(
        "a([bc]*)(c*d)",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,2) Group[2]=(3,1)",
    );
    t.run_regex_trial(
        "a([bc]+)(c*d)",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,2) Group[2]=(3,1)",
    );
    t.run_regex_trial(
        "a([bc]*)(c+d)",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,1) Group[2]=(2,2)",
    );
    t.run_regex_trial("a[bcd]*dcdcde", 0, "adcdcde", "Pass. Group[0]=(0,7)");
    t.run_regex_trial("a[bcd]+dcdcde", 0, "adcdcde", "Fail.");
    t.run_regex_trial("(ab|a)b*c", 0, "abc", "Pass. Group[0]=(0,3) Group[1]=(0,2)");
    t.run_regex_trial(
        "((a)(b)c)(d)",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(0,3) Group[2]=(0,1) Group[3]=(1,1) Group[4]=(3,1)",
    );
    t.run_regex_trial("[a-zA-Z_][a-zA-Z0-9_]*", 0, "alpha", "Pass. Group[0]=(0,5)");
    t.run_regex_trial(
        "^a(bc+|b[eh])g|.h$",
        0,
        "abh",
        "Pass. Group[0]=(1,2) Group[1]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        0,
        "effgz",
        "Pass. Group[0]=(0,5) Group[1]=(0,5) Group[2]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        0,
        "ij",
        "Pass. Group[0]=(0,2) Group[1]=(0,2) Group[2]=(1,1)",
    );
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", 0, "effg", "Fail.");
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", 0, "bcdd", "Fail.");
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        0,
        "reffgz",
        "Pass. Group[0]=(1,5) Group[1]=(1,5) Group[2]=",
    );
    t.run_regex_trial("((((((((((a))))))))))", 0, "a", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("((((((((((a))))))))))\\10", 0, "aa", "Pass. Group[0]=(0,2) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("((((((((((a))))))))))!", 0, "aa", "Fail.");
    t.run_regex_trial("((((((((((a))))))))))!", 0, "a!", "Pass. Group[0]=(0,2) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("(((((((((a)))))))))", 0, "a", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1)");
    t.run_regex_trial("multiple words of text", 0, "uh-uh", "Fail.");
    t.run_regex_trial(
        "multiple words",
        0,
        "multiple words, yeah",
        "Pass. Group[0]=(0,14)",
    );
    t.run_regex_trial(
        "(.*)c(.*)",
        0,
        "abcde",
        "Pass. Group[0]=(0,5) Group[1]=(0,2) Group[2]=(3,2)",
    );
    t.run_regex_trial(
        "\\((.*), (.*)\\)",
        0,
        "(a, b)",
        "Pass. Group[0]=(0,6) Group[1]=(1,1) Group[2]=(4,1)",
    );
    t.run_regex_trial("[k]", 0, "ab", "Fail.");
    t.run_regex_trial("abcd", 0, "abcd", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("a(bc)d", 0, "abcd", "Pass. Group[0]=(0,4) Group[1]=(1,2)");
    t.run_regex_trial("a[-]?c", 0, "ac", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "(abc)\\1",
        0,
        "abcabc",
        "Pass. Group[0]=(0,6) Group[1]=(0,3)",
    );
    t.run_regex_trial(
        "([a-c]*)\\1",
        0,
        "abcabc",
        "Pass. Group[0]=(0,6) Group[1]=(0,3)",
    );
    t.run_regex_trial("\\1", 0, "-", "Error.");
    t.run_regex_trial("\\2", 0, "-", "Error.");
    t.run_regex_trial("(a)|\\1", 0, "a", "Pass. Group[0]=(0,1) Group[1]=(0,1)");
    t.run_regex_trial("(a)|\\1", 0, "x", "Fail.");
    t.run_regex_trial("(a)|\\2", 0, "-", "Error.");
    t.run_regex_trial(
        "(([a-c])b*?\\2)*",
        0,
        "ababbbcbc",
        "Pass. Group[0]=(0,5) Group[1]=(0,3)(3,2) Group[2]=(0,1)(3,1)",
    );
    t.run_regex_trial(
        "(([a-c])b*?\\2){3}",
        0,
        "ababbbcbc",
        "Pass. Group[0]=(0,9) Group[1]=(0,3)(3,3)(6,3) Group[2]=(0,1)(3,1)(6,1)",
    );
    t.run_regex_trial("((\\3|b)\\2(a)x)+", 0, "aaxabxbaxbbx", "Fail.");
    t.run_regex_trial(
        "((\\3|b)\\2(a)x)+",
        0,
        "aaaxabaxbaaxbbax",
        "Pass. Group[0]=(12,4) Group[1]=(12,4) Group[2]=(12,1) Group[3]=(14,1)",
    );
    t.run_regex_trial("((\\3|b)\\2(a)){2,}", 0, "bbaababbabaaaaabbaaaabba", "Pass. Group[0]=(15,9) Group[1]=(15,3)(18,3)(21,3) Group[2]=(15,1)(18,1)(21,1) Group[3]=(17,1)(20,1)(23,1)");
    t.run_regex_trial("abc", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc", IGNORE_CASE, "XBC", "Fail.");
    t.run_regex_trial("abc", IGNORE_CASE, "AXC", "Fail.");
    t.run_regex_trial("abc", IGNORE_CASE, "ABX", "Fail.");
    t.run_regex_trial("abc", IGNORE_CASE, "XABCY", "Pass. Group[0]=(1,3)");
    t.run_regex_trial("abc", IGNORE_CASE, "ABABC", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("ab*c", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", IGNORE_CASE, "ABBC", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab*?bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{0,}?bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab+?bc", IGNORE_CASE, "ABBC", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab+bc", IGNORE_CASE, "ABC", "Fail.");
    t.run_regex_trial("ab+bc", IGNORE_CASE, "ABQ", "Fail.");
    t.run_regex_trial("ab{1,}bc", IGNORE_CASE, "ABQ", "Fail.");
    t.run_regex_trial("ab+bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,}?bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,3}?bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{3,4}?bc", IGNORE_CASE, "ABBBBC", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{4,5}?bc", IGNORE_CASE, "ABBBBC", "Fail.");
    t.run_regex_trial("ab??bc", IGNORE_CASE, "ABBC", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab??bc", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}?bc", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab??bc", IGNORE_CASE, "ABBBBC", "Fail.");
    t.run_regex_trial("ab??c", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}?c", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", IGNORE_CASE, "ABCC", "Fail.");
    t.run_regex_trial("^abc", IGNORE_CASE, "ABCC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", IGNORE_CASE, "AABC", "Fail.");
    t.run_regex_trial("abc$", IGNORE_CASE, "AABC", "Pass. Group[0]=(1,3)");
    t.run_regex_trial("^", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,0)");
    t.run_regex_trial("$", IGNORE_CASE, "ABC", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("a.c", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.c", IGNORE_CASE, "AXC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.*?c", IGNORE_CASE, "AXYZC", "Pass. Group[0]=(0,5)");
    t.run_regex_trial("a.*c", IGNORE_CASE, "AXYZD", "Fail.");
    t.run_regex_trial("a[bc]d", IGNORE_CASE, "ABC", "Fail.");
    t.run_regex_trial("a[bc]d", IGNORE_CASE, "ABD", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]e", IGNORE_CASE, "ABD", "Fail.");
    t.run_regex_trial("a[b-d]e", IGNORE_CASE, "ACE", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]", IGNORE_CASE, "AAC", "Pass. Group[0]=(1,2)");
    t.run_regex_trial("a[-b]", IGNORE_CASE, "A-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-]", IGNORE_CASE, "A-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-a]", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("a[]b", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("a[", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("a]", IGNORE_CASE, "A]", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[]]b", IGNORE_CASE, "A]B", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", IGNORE_CASE, "AED", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", IGNORE_CASE, "ABD", "Fail.");
    t.run_regex_trial("a[^-b]c", IGNORE_CASE, "ADC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^-b]c", IGNORE_CASE, "A-C", "Fail.");
    t.run_regex_trial("a[^]b]c", IGNORE_CASE, "A]C", "Fail.");
    t.run_regex_trial("a[^]b]c", IGNORE_CASE, "ADC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab|cd", IGNORE_CASE, "ABC", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("ab|cd", IGNORE_CASE, "ABCD", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "()ef",
        IGNORE_CASE,
        "DEF",
        "Pass. Group[0]=(1,2) Group[1]=(1,0)",
    );
    t.run_regex_trial("*a", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("(*)b", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("$b", IGNORE_CASE, "B", "Fail.");
    t.run_regex_trial("a\\", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("a\\(b", IGNORE_CASE, "A(B", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a\\(*b", IGNORE_CASE, "AB", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a\\(*b", IGNORE_CASE, "A((B", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("a\\\\b", IGNORE_CASE, "A\\B", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc)", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("(abc", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial(
        "((a))",
        IGNORE_CASE,
        "ABC",
        "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1)",
    );
    t.run_regex_trial(
        "(a)b(c)",
        IGNORE_CASE,
        "ABC",
        "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(2,1)",
    );
    t.run_regex_trial("a+b+c", IGNORE_CASE, "AABBABC", "Pass. Group[0]=(4,3)");
    t.run_regex_trial(
        "a{1,}b{1,}c",
        IGNORE_CASE,
        "AABBABC",
        "Pass. Group[0]=(4,3)",
    );
    t.run_regex_trial("a**", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("a.+?c", IGNORE_CASE, "ABCABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.*?c", IGNORE_CASE, "ABCABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.{0,5}?c", IGNORE_CASE, "ABCABC", "Pass. Group[0]=(0,3)");
    t.run_regex_trial(
        "(a+|b)*",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,}",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b)+",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b){1,}",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)(1,1)",
    );
    t.run_regex_trial(
        "(a+|b)?",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,1}",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,1}?",
        IGNORE_CASE,
        "AB",
        "Pass. Group[0]=(0,0) Group[1]=",
    );
    t.run_regex_trial(")(", IGNORE_CASE, "-", "Error.");
    t.run_regex_trial("[^ab]*", IGNORE_CASE, "CDE", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc", IGNORE_CASE, "", "Fail.");
    t.run_regex_trial("a*", IGNORE_CASE, "", "Pass. Group[0]=(0,0)");
    t.run_regex_trial(
        "([abc])*d",
        IGNORE_CASE,
        "ABBBCD",
        "Pass. Group[0]=(0,6) Group[1]=(0,1)(1,1)(2,1)(3,1)(4,1)",
    );
    t.run_regex_trial(
        "([abc])*bcd",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(0,1)",
    );
    t.run_regex_trial("a|b|c|d|e", IGNORE_CASE, "E", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "(a|b|c|d|e)f",
        IGNORE_CASE,
        "EF",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("abcd*efg", IGNORE_CASE, "ABCDEFG", "Pass. Group[0]=(0,7)");
    t.run_regex_trial("ab*", IGNORE_CASE, "XABYABBBZ", "Pass. Group[0]=(1,2)");
    t.run_regex_trial("ab*", IGNORE_CASE, "XAYABBBZ", "Pass. Group[0]=(1,1)");
    t.run_regex_trial(
        "(ab|cd)e",
        IGNORE_CASE,
        "ABCDE",
        "Pass. Group[0]=(2,3) Group[1]=(2,2)",
    );
    t.run_regex_trial("[abhgefdc]ij", IGNORE_CASE, "HIJ", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^(ab|cd)e", IGNORE_CASE, "ABCDE", "Fail.");
    t.run_regex_trial(
        "(abc|)ef",
        IGNORE_CASE,
        "ABCDEF",
        "Pass. Group[0]=(4,2) Group[1]=(4,0)",
    );
    t.run_regex_trial(
        "(a|b)c*d",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(1,3) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "(ab|ab*)bc",
        IGNORE_CASE,
        "ABC",
        "Pass. Group[0]=(0,3) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "a([bc]*)c*",
        IGNORE_CASE,
        "ABC",
        "Pass. Group[0]=(0,3) Group[1]=(1,2)",
    );
    t.run_regex_trial(
        "a([bc]*)(c*d)",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(1,2) Group[2]=(3,1)",
    );
    t.run_regex_trial(
        "a([bc]+)(c*d)",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(1,2) Group[2]=(3,1)",
    );
    t.run_regex_trial(
        "a([bc]*)(c+d)",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(1,1) Group[2]=(2,2)",
    );
    t.run_regex_trial(
        "a[bcd]*dcdcde",
        IGNORE_CASE,
        "ADCDCDE",
        "Pass. Group[0]=(0,7)",
    );
    t.run_regex_trial("a[bcd]+dcdcde", IGNORE_CASE, "ADCDCDE", "Fail.");
    t.run_regex_trial(
        "(ab|a)b*c",
        IGNORE_CASE,
        "ABC",
        "Pass. Group[0]=(0,3) Group[1]=(0,2)",
    );
    t.run_regex_trial(
        "((a)(b)c)(d)",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(0,3) Group[2]=(0,1) Group[3]=(1,1) Group[4]=(3,1)",
    );
    t.run_regex_trial(
        "[a-zA-Z_][a-zA-Z0-9_]*",
        IGNORE_CASE,
        "ALPHA",
        "Pass. Group[0]=(0,5)",
    );
    t.run_regex_trial(
        "^a(bc+|b[eh])g|.h$",
        IGNORE_CASE,
        "ABH",
        "Pass. Group[0]=(1,2) Group[1]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        IGNORE_CASE,
        "EFFGZ",
        "Pass. Group[0]=(0,5) Group[1]=(0,5) Group[2]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        IGNORE_CASE,
        "IJ",
        "Pass. Group[0]=(0,2) Group[1]=(0,2) Group[2]=(1,1)",
    );
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", IGNORE_CASE, "EFFG", "Fail.");
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", IGNORE_CASE, "BCDD", "Fail.");
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        IGNORE_CASE,
        "REFFGZ",
        "Pass. Group[0]=(1,5) Group[1]=(1,5) Group[2]=",
    );
    t.run_regex_trial("((((((((((a))))))))))", IGNORE_CASE, "A", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("((((((((((a))))))))))\\10", IGNORE_CASE, "AA", "Pass. Group[0]=(0,2) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("((((((((((a))))))))))!", IGNORE_CASE, "AA", "Fail.");
    t.run_regex_trial("((((((((((a))))))))))!", IGNORE_CASE, "A!", "Pass. Group[0]=(0,2) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("(((((((((a)))))))))", IGNORE_CASE, "A", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1)");
    t.run_regex_trial(
        "(?:(?:(?:(?:(?:(?:(?:(?:(?:(a))))))))))",
        IGNORE_CASE,
        "A",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "(?:(?:(?:(?:(?:(?:(?:(?:(?:(a|b|c))))))))))",
        IGNORE_CASE,
        "C",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial("multiple words of text", IGNORE_CASE, "UH-UH", "Fail.");
    t.run_regex_trial(
        "multiple words",
        IGNORE_CASE,
        "MULTIPLE WORDS, YEAH",
        "Pass. Group[0]=(0,14)",
    );
    t.run_regex_trial(
        "(.*)c(.*)",
        IGNORE_CASE,
        "ABCDE",
        "Pass. Group[0]=(0,5) Group[1]=(0,2) Group[2]=(3,2)",
    );
    t.run_regex_trial(
        "\\((.*), (.*)\\)",
        IGNORE_CASE,
        "(A, B)",
        "Pass. Group[0]=(0,6) Group[1]=(1,1) Group[2]=(4,1)",
    );
    t.run_regex_trial("[k]", IGNORE_CASE, "AB", "Fail.");
    t.run_regex_trial("abcd", IGNORE_CASE, "ABCD", "Pass. Group[0]=(0,4)");
    t.run_regex_trial(
        "a(bc)d",
        IGNORE_CASE,
        "ABCD",
        "Pass. Group[0]=(0,4) Group[1]=(1,2)",
    );
    t.run_regex_trial("a[-]?c", IGNORE_CASE, "AC", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "(abc)\\1",
        IGNORE_CASE,
        "ABCABC",
        "Pass. Group[0]=(0,6) Group[1]=(0,3)",
    );
    t.run_regex_trial(
        "([a-c]*)\\1",
        IGNORE_CASE,
        "ABCABC",
        "Pass. Group[0]=(0,6) Group[1]=(0,3)",
    );
    t.run_regex_trial("a(?!b).", 0, "abad", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("a(?=d).", 0, "abad", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("a(?=c|d).", 0, "abad", "Pass. Group[0]=(2,2)");
    t.run_regex_trial(
        "a(?:b|c|d)(.)",
        0,
        "ace",
        "Pass. Group[0]=(0,3) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d)*(.)",
        0,
        "ace",
        "Pass. Group[0]=(0,3) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d)+?(.)",
        0,
        "ace",
        "Pass. Group[0]=(0,3) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d)+?(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,3) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d)+(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,8) Group[1]=(7,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){2}(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,4) Group[1]=(3,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){4,5}(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,7) Group[1]=(6,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){4,5}?(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,6) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        "((foo)|(bar))*",
        0,
        "foobar",
        "Pass. Group[0]=(0,6) Group[1]=(0,3)(3,3) Group[2]=(0,3) Group[3]=(3,3)",
    );
    t.run_regex_trial(":(?:", 0, "-", "Error.");
    t.run_regex_trial(
        "a(?:b|c|d){6,7}(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,8) Group[1]=(7,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){6,7}?(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,8) Group[1]=(7,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){5,6}(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,8) Group[1]=(7,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){5,6}?(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,7) Group[1]=(6,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){5,7}(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,8) Group[1]=(7,1)",
    );
    t.run_regex_trial(
        "a(?:b|c|d){5,7}?(.)",
        0,
        "acdbcdbe",
        "Pass. Group[0]=(0,7) Group[1]=(6,1)",
    );
    t.run_regex_trial(
        "a(?:b|(c|e){1,2}?|d)+?(.)",
        0,
        "ace",
        "Pass. Group[0]=(0,3) Group[1]=(1,1) Group[2]=(2,1)",
    );
    t.run_regex_trial("^(.+)?B", 0, "AB", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial(
        "^([^a-z])|(\\^)$",
        0,
        ".",
        "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=",
    );
    t.run_regex_trial("^[<>]&", 0, "<&OUT", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "^(a\\1?){4}$",
        0,
        "aaaaaaaaaa",
        "Pass. Group[0]=(0,10) Group[1]=(0,1)(1,2)(3,3)(6,4)",
    );
    t.run_regex_trial("^(a\\1?){4}$", 0, "aaaaaaaaa", "Fail.");
    t.run_regex_trial("^(a\\1?){4}$", 0, "aaaaaaaaaaa", "Fail.");
    t.run_regex_trial(
        "^(a(?(1)\\1)){4}$",
        0,
        "aaaaaaaaaa",
        "Pass. Group[0]=(0,10) Group[1]=(0,1)(1,2)(3,3)(6,4)",
    );
    t.run_regex_trial("^(a(?(1)\\1)){4}$", 0, "aaaaaaaaa", "Fail.");
    t.run_regex_trial("^(a(?(1)\\1)){4}$", 0, "aaaaaaaaaaa", "Fail.");
    t.run_regex_trial(
        "((a{4})+)",
        0,
        "aaaaaaaaa",
        "Pass. Group[0]=(0,8) Group[1]=(0,8) Group[2]=(0,4)(4,4)",
    );
    t.run_regex_trial(
        "(((aa){2})+)",
        0,
        "aaaaaaaaaa",
        "Pass. Group[0]=(0,8) Group[1]=(0,8) Group[2]=(0,4)(4,4) Group[3]=(0,2)(2,2)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "(((a{2}){2})+)",
        0,
        "aaaaaaaaaa",
        "Pass. Group[0]=(0,8) Group[1]=(0,8) Group[2]=(0,4)(4,4) Group[3]=(0,2)(2,2)(4,2)(6,2)",
    );
    t.run_regex_trial("(?:(f)(o)(o)|(b)(a)(r))*", 0, "foobar", "Pass. Group[0]=(0,6) Group[1]=(0,1) Group[2]=(1,1) Group[3]=(2,1) Group[4]=(3,1) Group[5]=(4,1) Group[6]=(5,1)");
    t.run_regex_trial("(?<=a)b", 0, "ab", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("(?<=a)b", 0, "cb", "Fail.");
    t.run_regex_trial("(?<=a)b", 0, "b", "Fail.");
    t.run_regex_trial("(?<!c)b", 0, "ab", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("(?<!c)b", 0, "cb", "Fail.");
    t.run_regex_trial("(?<!c)b", 0, "b", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("(?<!c)b", 0, "b", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("(?<%)b", 0, "-", "Error.");
    t.run_regex_trial("(?:..)*a", 0, "aba", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("(?:..)*?a", 0, "aba", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "^(?:b|a(?=(.)))*\\1",
        0,
        "abc",
        "Pass. Group[0]=(0,2) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "^(){3,5}",
        0,
        "abc",
        "Pass. Group[0]=(0,0) Group[1]=(0,0)(0,0)(0,0)",
    );
    t.run_regex_trial("^(a+)*ax", 0, "aax", "Pass. Group[0]=(0,3) Group[1]=(0,1)");
    t.run_regex_trial(
        "^((a|b)+)*ax",
        0,
        "aax",
        "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(0,1)",
    );
    t.run_regex_trial(
        "^((a|bc)+)*ax",
        0,
        "aax",
        "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(0,1)",
    );
    t.run_regex_trial("(a|x)*ab", 0, "cab", "Pass. Group[0]=(1,2) Group[1]=");
    t.run_regex_trial("(a)*ab", 0, "cab", "Pass. Group[0]=(1,2) Group[1]=");
    t.run_regex_trial("(?:(?i)a)b", 0, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("((?i)a)b", 0, "ab", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial("(?:(?i)a)b", 0, "Ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("((?i)a)b", 0, "Ab", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial("(?:(?i)a)b", 0, "aB", "Fail.");
    t.run_regex_trial("((?i)a)b", 0, "aB", "Fail.");
    t.run_regex_trial("(?i:a)b", 0, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("((?i:a))b", 0, "ab", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial("(?i:a)b", 0, "Ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("((?i:a))b", 0, "Ab", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial("(?i:a)b", 0, "aB", "Fail.");
    t.run_regex_trial("((?i:a))b", 0, "aB", "Fail.");
    t.run_regex_trial("(?:(?-i)a)b", IGNORE_CASE, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i)a)b",
        IGNORE_CASE,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?:(?-i)a)b", IGNORE_CASE, "aB", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i)a)b",
        IGNORE_CASE,
        "aB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?:(?-i)a)b", IGNORE_CASE, "Ab", "Fail.");
    t.run_regex_trial("((?-i)a)b", IGNORE_CASE, "Ab", "Fail.");
    t.run_regex_trial("(?:(?-i)a)b", IGNORE_CASE, "aB", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i)a)b",
        IGNORE_CASE,
        "aB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?:(?-i)a)b", IGNORE_CASE, "AB", "Fail.");
    t.run_regex_trial("((?-i)a)b", IGNORE_CASE, "AB", "Fail.");
    t.run_regex_trial("(?-i:a)b", IGNORE_CASE, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i:a))b",
        IGNORE_CASE,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?-i:a)b", IGNORE_CASE, "aB", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i:a))b",
        IGNORE_CASE,
        "aB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?-i:a)b", IGNORE_CASE, "Ab", "Fail.");
    t.run_regex_trial("((?-i:a))b", IGNORE_CASE, "Ab", "Fail.");
    t.run_regex_trial("(?-i:a)b", IGNORE_CASE, "aB", "Pass. Group[0]=(0,2)");
    t.run_regex_trial(
        "((?-i:a))b",
        IGNORE_CASE,
        "aB",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("(?-i:a)b", IGNORE_CASE, "AB", "Fail.");
    t.run_regex_trial("((?-i:a))b", IGNORE_CASE, "AB", "Fail.");
    t.run_regex_trial("((?-i:a.))b", IGNORE_CASE, "a\u{a}B", "Fail.");
    t.run_regex_trial(
        "((?s-i:a.))b",
        IGNORE_CASE,
        "a\u{a}B",
        "Pass. Group[0]=(0,3) Group[1]=(0,2)",
    );
    t.run_regex_trial("((?s-i:a.))b", IGNORE_CASE, "B\u{a}B", "Fail.");
    t.run_regex_trial(
        "(?:c|d)(?:)(?:a(?:)(?:b)(?:b(?:))(?:b(?:)(?:b)))",
        0,
        "cabbbb",
        "Pass. Group[0]=(0,6)",
    );
    t.run_regex_trial(
        "(?:c|d)(?:)(?:aaaaaaaa(?:)(?:bbbbbbbb)(?:bbbbbbbb(?:))(?:bbbbbbbb(?:)(?:bbbbbbbb)))",
        0,
        "caaaaaaaabbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "Pass. Group[0]=(0,41)",
    );
    t.run_regex_trial(
        "(ab)\\d\\1",
        IGNORE_CASE,
        "Ab4ab",
        "Pass. Group[0]=(0,5) Group[1]=(0,2)",
    );
    t.run_regex_trial(
        "(ab)\\d\\1",
        IGNORE_CASE,
        "ab4Ab",
        "Pass. Group[0]=(0,5) Group[1]=(0,2)",
    );
    t.run_regex_trial(
        "foo\\w*\\d{4}baz",
        0,
        "foobar1234baz",
        "Pass. Group[0]=(0,13)",
    );
    t.run_regex_trial(
        "x(~~)*(?:(?:F)?)?",
        0,
        "x~~",
        "Pass. Group[0]=(0,3) Group[1]=(1,2)",
    );
    t.run_regex_trial("^a(?#xxx){3}c", 0, "aaac", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("(?<![cd])b", 0, "dbcb", "Fail.");
    t.run_regex_trial("(?<![cd])[ab]", 0, "dbaacb", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("(?<!(c|d))b", 0, "dbcb", "Fail.");
    t.run_regex_trial(
        "(?<!(c|d))[ab]",
        0,
        "dbaacb",
        "Pass. Group[0]=(2,1) Group[1]=",
    );
    t.run_regex_trial("(?<!cd)[ab]", 0, "cdaccb", "Pass. Group[0]=(5,1)");
    t.run_regex_trial("^(?:a?b?)*$", 0, "a--", "Fail.");
    t.run_regex_trial(
        "((?s)^a(.))((?m)^b$)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(0,3) Group[1]=(0,2) Group[2]=(1,1) Group[3]=(2,1)",
    );
    t.run_regex_trial(
        "((?m)^b$)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(2,1) Group[1]=(2,1)",
    );
    t.run_regex_trial("(?m)^b", 0, "a\u{a}b\u{a}", "Pass. Group[0]=(2,1)");
    t.run_regex_trial(
        "(?m)^(b)",
        0,
        "a\u{a}b\u{a}",
        "Pass. Group[0]=(2,1) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "((?m)^b)",
        0,
        "a\u{a}b\u{a}",
        "Pass. Group[0]=(2,1) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "\\n((?m)^b)",
        0,
        "a\u{a}b\u{a}",
        "Pass. Group[0]=(1,2) Group[1]=(2,1)",
    );
    t.run_regex_trial(
        "((?s).)c(?!.)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(3,2) Group[1]=(3,1)",
    );
    t.run_regex_trial(
        "((?s).)c(?!.)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(3,2) Group[1]=(3,1)",
    );
    t.run_regex_trial(
        "((?s)b.)c(?!.)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(2,3) Group[1]=(2,2)",
    );
    t.run_regex_trial(
        "((?s)b.)c(?!.)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(2,3) Group[1]=(2,2)",
    );
    t.run_regex_trial("^b", 0, "a\u{a}b\u{a}c\u{a}", "Fail.");
    t.run_regex_trial("()^b", 0, "a\u{a}b\u{a}c\u{a}", "Fail.");
    t.run_regex_trial(
        "((?m)^b)",
        0,
        "a\u{a}b\u{a}c\u{a}",
        "Pass. Group[0]=(2,1) Group[1]=(2,1)",
    );
    t.run_regex_trial("(x)?(?(1)a|b)", 0, "a", "Fail.");
    t.run_regex_trial("(x)?(?(1)b|a)", 0, "a", "Pass. Group[0]=(0,1) Group[1]=");
    t.run_regex_trial("()?(?(1)b|a)", 0, "a", "Pass. Group[0]=(0,1) Group[1]=");
    t.run_regex_trial("()(?(1)b|a)", 0, "a", "Fail.");
    t.run_regex_trial(
        "()?(?(1)a|b)",
        0,
        "a",
        "Pass. Group[0]=(0,1) Group[1]=(0,0)",
    );
    t.run_regex_trial(
        "^(\\()?blah(?(1)(\\)))$",
        0,
        "(blah)",
        "Pass. Group[0]=(0,6) Group[1]=(0,1) Group[2]=(5,1)",
    );
    t.run_regex_trial(
        "^(\\()?blah(?(1)(\\)))$",
        0,
        "blah",
        "Pass. Group[0]=(0,4) Group[1]= Group[2]=",
    );
    t.run_regex_trial("^(\\()?blah(?(1)(\\)))$", 0, "blah)", "Fail.");
    t.run_regex_trial("^(\\()?blah(?(1)(\\)))$", 0, "(blah", "Fail.");
    t.run_regex_trial(
        "^(\\(+)?blah(?(1)(\\)))$",
        0,
        "(blah)",
        "Pass. Group[0]=(0,6) Group[1]=(0,1) Group[2]=(5,1)",
    );
    t.run_regex_trial(
        "^(\\(+)?blah(?(1)(\\)))$",
        0,
        "blah",
        "Pass. Group[0]=(0,4) Group[1]= Group[2]=",
    );
    t.run_regex_trial("^(\\(+)?blah(?(1)(\\)))$", 0, "blah)", "Fail.");
    t.run_regex_trial("^(\\(+)?blah(?(1)(\\)))$", 0, "(blah", "Fail.");
    t.run_regex_trial("(?(1)a|b|c)", 0, "a", "Error.");
    t.run_regex_trial("(?(?!a)a|b)", 0, "a", "Fail.");
    t.run_regex_trial("(?(?!a)b|a)", 0, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("(?(?=a)b|a)", 0, "a", "Fail.");
    t.run_regex_trial("(?(?=a)a|b)", 0, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "(?=(a+?))(\\1ab)",
        0,
        "aaab",
        "Pass. Group[0]=(1,3) Group[1]=(1,1) Group[2]=(1,3)",
    );
    t.run_regex_trial("^(?=(a+?))\\1ab", 0, "aaab", "Fail.");
    t.run_regex_trial("(\\w+:)+", 0, "one:", "Pass. Group[0]=(0,4) Group[1]=(0,4)");
    t.run_regex_trial("$(?<=^(a))", 0, "a", "Pass. Group[0]=(1,0) Group[1]=(0,1)");
    t.run_regex_trial(
        "(?=(a+?))(\\1ab)",
        0,
        "aaab",
        "Pass. Group[0]=(1,3) Group[1]=(1,1) Group[2]=(1,3)",
    );
    t.run_regex_trial("^(?=(a+?))\\1ab", 0, "aaab", "Fail.");
    t.run_regex_trial("([\\w:]+::)?(\\w+)$", 0, "abcd:", "Fail.");
    t.run_regex_trial(
        "([\\w:]+::)?(\\w+)$",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]= Group[2]=(0,4)",
    );
    t.run_regex_trial(
        "([\\w:]+::)?(\\w+)$",
        0,
        "xy:z:::abcd",
        "Pass. Group[0]=(0,11) Group[1]=(0,7) Group[2]=(7,4)",
    );
    t.run_regex_trial(
        "^[^bcd]*(c+)",
        0,
        "aexycd",
        "Pass. Group[0]=(0,5) Group[1]=(4,1)",
    );
    t.run_regex_trial("(a*)b+", 0, "caab", "Pass. Group[0]=(1,3) Group[1]=(1,2)");
    t.run_regex_trial("([\\w:]+::)?(\\w+)$", 0, "abcd:", "Fail.");
    t.run_regex_trial(
        "([\\w:]+::)?(\\w+)$",
        0,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]= Group[2]=(0,4)",
    );
    t.run_regex_trial(
        "([\\w:]+::)?(\\w+)$",
        0,
        "xy:z:::abcd",
        "Pass. Group[0]=(0,11) Group[1]=(0,7) Group[2]=(7,4)",
    );
    t.run_regex_trial(
        "^[^bcd]*(c+)",
        0,
        "aexycd",
        "Pass. Group[0]=(0,5) Group[1]=(4,1)",
    );
    t.run_regex_trial("(>a+)ab", 0, "aaab", "Fail.");
    t.run_regex_trial("(?>a+)b", 0, "aaab", "Pass. Group[0]=(0,4)");
    t.run_regex_trial(
        "([[:]+)",
        0,
        "a:[b]:",
        "Pass. Group[0]=(1,2) Group[1]=(1,2)",
    );
    t.run_regex_trial(
        "([[=]+)",
        0,
        "a=[b]=",
        "Pass. Group[0]=(1,2) Group[1]=(1,2)",
    );
    t.run_regex_trial(
        "([[.]+)",
        0,
        "a.[b].",
        "Pass. Group[0]=(1,2) Group[1]=(1,2)",
    );
    t.run_regex_trial("[a[:]b[:c]", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("[a[:]b[:c]", 0, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial(
        "((?>a+)b)",
        0,
        "aaab",
        "Pass. Group[0]=(0,4) Group[1]=(0,4)",
    );
    t.run_regex_trial(
        "(?>(a+))b",
        0,
        "aaab",
        "Pass. Group[0]=(0,4) Group[1]=(0,3)",
    );
    t.run_regex_trial(
        "((?>[^()]+)|\\([^()]*\\))+",
        0,
        "((abc(ade)ufh()()x",
        "Pass. Group[0]=(2,16) Group[1]=(2,3)(5,5)(10,3)(13,2)(15,2)(17,1)",
    );
    t.run_regex_trial("(?<=x+)", 0, "xxxxy", "Pass. Group[0]=(1,0)");
    t.run_regex_trial("a{37,17}", 0, "-", "Error.");
    t.run_regex_trial("\\Z", 0, "a\u{a}b\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", 0, "a\u{a}b\u{a}", "Pass. Group[0]=(4,0)");
    t.run_regex_trial("$", 0, "a\u{a}b\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\Z", 0, "b\u{a}a\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", 0, "b\u{a}a\u{a}", "Pass. Group[0]=(4,0)");
    t.run_regex_trial("$", 0, "b\u{a}a\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\Z", 0, "b\u{a}a", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", 0, "b\u{a}a", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("$", 0, "b\u{a}a", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\Z", MULTILINE, "a\u{a}b\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", MULTILINE, "a\u{a}b\u{a}", "Pass. Group[0]=(4,0)");
    t.run_regex_trial("$", MULTILINE, "a\u{a}b\u{a}", "Pass. Group[0]=(1,0)");
    t.run_regex_trial("\\Z", MULTILINE, "b\u{a}a\u{a}", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", MULTILINE, "b\u{a}a\u{a}", "Pass. Group[0]=(4,0)");
    t.run_regex_trial("$", MULTILINE, "b\u{a}a\u{a}", "Pass. Group[0]=(1,0)");
    t.run_regex_trial("\\Z", MULTILINE, "b\u{a}a", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("\\z", MULTILINE, "b\u{a}a", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("$", MULTILINE, "b\u{a}a", "Pass. Group[0]=(1,0)");
    t.run_regex_trial("a\\Z", 0, "a\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("a\\z", 0, "a\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("a$", 0, "a\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("a\\Z", 0, "b\u{a}a\u{a}", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\z", 0, "b\u{a}a\u{a}", "Fail.");
    t.run_regex_trial("a$", 0, "b\u{a}a\u{a}", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\Z", 0, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\z", 0, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a$", 0, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\z", MULTILINE, "a\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("a$", MULTILINE, "a\u{a}b\u{a}", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a\\Z", MULTILINE, "b\u{a}a\u{a}", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\z", MULTILINE, "b\u{a}a\u{a}", "Fail.");
    t.run_regex_trial("a$", MULTILINE, "b\u{a}a\u{a}", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\Z", MULTILINE, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a\\z", MULTILINE, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("a$", MULTILINE, "b\u{a}a", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("aa\\Z", 0, "aa\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", 0, "aa\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "aa\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}aa\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\z", 0, "b\u{a}aa\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "b\u{a}aa\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\z", 0, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa$", 0, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\z", MULTILINE, "aa\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "aa\u{a}b\u{a}", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}aa\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}aa\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}aa\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}aa", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("aa\\Z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa\\z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa$", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa$", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("aa\\z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("aa$", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("aa\\Z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("aa\\z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("aa$", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "ab\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", 0, "ab\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "ab\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ab\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ab\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "b\u{a}ab\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab$", 0, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\z", MULTILINE, "ab\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "ab\u{a}b\u{a}", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ab\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ab\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ab\u{a}", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ab", "Pass. Group[0]=(2,2)");
    t.run_regex_trial("ab\\Z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab$", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab$", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab\\z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab$", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("ab\\Z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab\\z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("ab$", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "abb\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", 0, "abb\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "abb\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}abb\u{a}", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\z", 0, "b\u{a}abb\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "b\u{a}abb\u{a}", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\z", 0, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb$", 0, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\z", MULTILINE, "abb\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "abb\u{a}b\u{a}", "Pass. Group[0]=(0,3)");
    t.run_regex_trial(
        "abb\\Z",
        MULTILINE,
        "b\u{a}abb\u{a}",
        "Pass. Group[0]=(2,3)",
    );
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}abb\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}abb\u{a}", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\Z", MULTILINE, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}abb", "Pass. Group[0]=(2,3)");
    t.run_regex_trial("abb\\Z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb\\z", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb$", 0, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "ac\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}ac\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}ac", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb$", 0, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb\\z", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb$", 0, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "ca\u{a}b\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}ca\u{a}", "Fail.");
    t.run_regex_trial("abb\\Z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb\\z", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial("abb$", MULTILINE, "b\u{a}ca", "Fail.");
    t.run_regex_trial(
        "(^|x)(c)",
        0,
        "ca",
        "Pass. Group[0]=(0,1) Group[1]=(0,0) Group[2]=(0,1)",
    );
    t.run_regex_trial(
        "a*abc?xyz+pqr{3}ab{2,}xy{4,5}pq{0,6}AB{0,}zz",
        0,
        "x",
        "Fail.",
    );
    t.run_regex_trial(
        "round\\(((?>[^()]+))\\)",
        0,
        "_I(round(xs * sz),1)",
        "Pass. Group[0]=(3,14) Group[1]=(9,7)",
    );
    t.run_regex_trial("foo.bart", 0, "foo.bart", "Pass. Group[0]=(0,8)");
    t.run_regex_trial(
        "^d[x][x][x]",
        MULTILINE,
        "abcd\u{a}dxxx",
        "Pass. Group[0]=(5,4)",
    );
    t.run_regex_trial(
        ".X(.+)+X",
        0,
        "bbbbXcXaaaaaaaa",
        "Pass. Group[0]=(3,4) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".X(.+)+XX",
        0,
        "bbbbXcXXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".XX(.+)+X",
        0,
        "bbbbXXcXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(6,1)",
    );
    t.run_regex_trial(".X(.+)+X", 0, "bbbbXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".X(.+)+XX", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".XX(.+)+X", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(
        ".X(.+)+[X]",
        0,
        "bbbbXcXaaaaaaaa",
        "Pass. Group[0]=(3,4) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".X(.+)+[X][X]",
        0,
        "bbbbXcXXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".XX(.+)+[X]",
        0,
        "bbbbXXcXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(6,1)",
    );
    t.run_regex_trial(".X(.+)+[X]", 0, "bbbbXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".X(.+)+[X][X]", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".XX(.+)+[X]", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(
        ".[X](.+)+[X]",
        0,
        "bbbbXcXaaaaaaaa",
        "Pass. Group[0]=(3,4) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".[X](.+)+[X][X]",
        0,
        "bbbbXcXXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(5,1)",
    );
    t.run_regex_trial(
        ".[X][X](.+)+[X]",
        0,
        "bbbbXXcXaaaaaaaa",
        "Pass. Group[0]=(3,5) Group[1]=(6,1)",
    );
    t.run_regex_trial(".[X](.+)+[X]", 0, "bbbbXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".[X](.+)+[X][X]", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial(".[X][X](.+)+[X]", 0, "bbbbXXXaaaaaaaaa", "Fail.");
    t.run_regex_trial("tt+$", 0, "xxxtt", "Pass. Group[0]=(3,2)");
    t.run_regex_trial(
        "([\\d-z]+)",
        0,
        "a0-za",
        "Pass. Group[0]=(1,3) Group[1]=(1,3)",
    );
    t.run_regex_trial(
        "([\\d-\\s]+)",
        0,
        "a0- z",
        "Pass. Group[0]=(1,3) Group[1]=(1,3)",
    );
    t.run_regex_trial("\\GX.*X", 0, "aaaXbX", "Fail.");
    t.run_regex_trial(
        "(\\d+\\.\\d+)",
        0,
        "3.1415926",
        "Pass. Group[0]=(0,9) Group[1]=(0,9)",
    );
    t.run_regex_trial(
        "(\\ba.{0,10}br)",
        0,
        "have a web browser",
        "Pass. Group[0]=(5,8) Group[1]=(5,8)",
    );
    t.run_regex_trial("\\.c(pp|xx|c)?$", IGNORE_CASE, "Changes", "Fail.");
    t.run_regex_trial(
        "\\.c(pp|xx|c)?$",
        IGNORE_CASE,
        "IO.c",
        "Pass. Group[0]=(2,2) Group[1]=",
    );
    t.run_regex_trial(
        "(\\.c(pp|xx|c)?$)",
        IGNORE_CASE,
        "IO.c",
        "Pass. Group[0]=(2,2) Group[1]=(2,2) Group[2]=",
    );
    t.run_regex_trial("^([a-z]:)", 0, "C:/", "Fail.");
    t.run_regex_trial(
        "^\\S\\s+aa$",
        MULTILINE,
        "\u{a}x aa",
        "Pass. Group[0]=(1,4)",
    );
    t.run_regex_trial("(^|a)b", 0, "ab", "Pass. Group[0]=(0,2) Group[1]=(0,1)");
    t.run_regex_trial(
        "^([ab]*?)(b)?(c)$",
        0,
        "abac",
        "Pass. Group[0]=(0,4) Group[1]=(0,3) Group[2]= Group[3]=(3,1)",
    );
    t.run_regex_trial("(\\w)?(abc)\\1b", 0, "abcab", "Fail.");
    t.run_regex_trial("^(?:.,){2}c", 0, "a,b,c", "Pass. Group[0]=(0,5)");
    t.run_regex_trial(
        "^(.,){2}c",
        0,
        "a,b,c",
        "Pass. Group[0]=(0,5) Group[1]=(0,2)(2,2)",
    );
    t.run_regex_trial("^(?:[^,]*,){2}c", 0, "a,b,c", "Pass. Group[0]=(0,5)");
    t.run_regex_trial(
        "^([^,]*,){2}c",
        0,
        "a,b,c",
        "Pass. Group[0]=(0,5) Group[1]=(0,2)(2,2)",
    );
    t.run_regex_trial(
        "^([^,]*,){3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]*,){3,}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]*,){0,3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,3},){3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,3},){3,}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,3},){0,3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,},){3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,},){3,}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{1,},){0,3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{0,3},){3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{0,3},){3,}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial(
        "^([^,]{0,3},){0,3}d",
        0,
        "aaa,b,c,d",
        "Pass. Group[0]=(0,9) Group[1]=(0,4)(4,2)(6,2)",
    );
    t.run_regex_trial("(?i)", 0, "", "Pass. Group[0]=(0,0)");
    t.run_regex_trial(
        "(?!\\A)x",
        MULTILINE,
        "a\u{a}xb\u{a}",
        "Pass. Group[0]=(2,1)",
    );
    t.run_regex_trial(
        "^(a(b)?)+$",
        0,
        "aba",
        "Pass. Group[0]=(0,3) Group[1]=(0,2)(2,1) Group[2]=(1,1)",
    );
    t.run_regex_trial(
        "^(aa(bb)?)+$",
        0,
        "aabbaa",
        "Pass. Group[0]=(0,6) Group[1]=(0,4)(4,2) Group[2]=(2,2)",
    );
    t.run_regex_trial(
        "^.{9}abc.*\\n",
        MULTILINE,
        "123\u{a}abcabcabcabc\u{a}",
        "Pass. Group[0]=(4,13)",
    );
    t.run_regex_trial("^(a)?a$", 0, "a", "Pass. Group[0]=(0,1) Group[1]=");
    t.run_regex_trial("^(a)?(?(1)a|b)+$", 0, "a", "Fail.");
    t.run_regex_trial(
        "^(a\\1?)(a\\1?)(a\\2?)(a\\3?)$",
        0,
        "aaaaaa",
        "Pass. Group[0]=(0,6) Group[1]=(0,1) Group[2]=(1,2) Group[3]=(3,1) Group[4]=(4,2)",
    );
    t.run_regex_trial(
        "^(a\\1?){4}$",
        0,
        "aaaaaa",
        "Pass. Group[0]=(0,6) Group[1]=(0,1)(1,2)(3,1)(4,2)",
    );
    t.run_regex_trial(
        "^(0+)?(?:x(1))?",
        0,
        "x1",
        "Pass. Group[0]=(0,2) Group[1]= Group[2]=(1,1)",
    );
    t.run_regex_trial(
        "^([0-9a-fA-F]+)(?:x([0-9a-fA-F]+)?)(?:x([0-9a-fA-F]+))?",
        0,
        "012cxx0190",
        "Pass. Group[0]=(0,10) Group[1]=(0,4) Group[2]= Group[3]=(6,4)",
    );
    t.run_regex_trial(
        "^(b+?|a){1,2}c",
        0,
        "bbbac",
        "Pass. Group[0]=(0,5) Group[1]=(0,3)(3,1)",
    );
    t.run_regex_trial(
        "^(b+?|a){1,2}c",
        0,
        "bbbbac",
        "Pass. Group[0]=(0,6) Group[1]=(0,4)(4,1)",
    );
    t.run_regex_trial(
        "\\((\\w\\. \\w+)\\)",
        0,
        "cd. (A. Tw)",
        "Pass. Group[0]=(4,7) Group[1]=(5,5)",
    );
    t.run_regex_trial(
        "((?:aaaa|bbbb)cccc)?",
        0,
        "aaaacccc",
        "Pass. Group[0]=(0,8) Group[1]=(0,8)",
    );
    t.run_regex_trial(
        "((?:aaaa|bbbb)cccc)?",
        0,
        "bbbbcccc",
        "Pass. Group[0]=(0,8) Group[1]=(0,8)",
    );
    t.run_regex_trial(
        "^(foo)|(bar)$",
        0,
        "foobar",
        "Pass. Group[0]=(0,3) Group[1]=(0,3) Group[2]=",
    );
    t.run_regex_trial(
        "^(foo)|(bar)$",
        RIGHT_TO_LEFT,
        "foobar",
        "Pass. Group[0]=(3,3) Group[1]= Group[2]=(3,3)",
    );
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("bab", RIGHT_TO_LEFT, "babababaa", "Pass. Group[0]=(4,3)");
    t.run_regex_trial("abb", RIGHT_TO_LEFT, "abb", "Pass. Group[0]=(0,3)");
    t.run_regex_trial(
        "b$",
        RIGHT_TO_LEFT | MULTILINE,
        "aab\u{a}aab",
        "Pass. Group[0]=(6,1)",
    );
    t.run_regex_trial(
        "^a",
        RIGHT_TO_LEFT | MULTILINE,
        "aab\u{a}aab",
        "Pass. Group[0]=(4,1)",
    );
    t.run_regex_trial(
        "^aaab",
        RIGHT_TO_LEFT | MULTILINE,
        "aaab\u{a}aab",
        "Pass. Group[0]=(0,4)",
    );
    t.run_regex_trial("abb{2}", RIGHT_TO_LEFT, "abbb", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("abb{1,2}", RIGHT_TO_LEFT, "abbb", "Pass. Group[0]=(0,4)");
    t.run_regex_trial(
        "abb{1,2}",
        RIGHT_TO_LEFT,
        "abbbbbaaaa",
        "Pass. Group[0]=(0,4)",
    );
    t.run_regex_trial("\\Ab", RIGHT_TO_LEFT, "bab\u{a}aaa", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\Abab$", RIGHT_TO_LEFT, "bab", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("b\\Z", RIGHT_TO_LEFT, "bab\u{a}aaa", "Fail.");
    t.run_regex_trial("b\\Z", RIGHT_TO_LEFT, "babaaab", "Pass. Group[0]=(6,1)");
    t.run_regex_trial("b\\z", RIGHT_TO_LEFT, "babaaa", "Fail.");
    t.run_regex_trial("b\\z", RIGHT_TO_LEFT, "babaaab", "Pass. Group[0]=(6,1)");
    t.run_regex_trial("a\\G", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(5,1)");
    t.run_regex_trial("\\Abaaa\\G", RIGHT_TO_LEFT, "baaa", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial(
        "\\bc",
        RIGHT_TO_LEFT,
        "aaa c aaa c a",
        "Pass. Group[0]=(10,1)",
    );
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "c aaa c", "Pass. Group[0]=(6,1)");
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "aaa ac", "Fail.");
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "c aaa", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "aaacaaa", "Fail.");
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "aaac aaa", "Fail.");
    t.run_regex_trial("\\bc", RIGHT_TO_LEFT, "aaa ca caaa", "Pass. Group[0]=(7,1)");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "ac aaa ac", "Pass. Group[0]=(8,1)");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "aaa c", "Fail.");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "ca aaa", "Fail.");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "aaa c aaa", "Fail.");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, " acaca ", "Pass. Group[0]=(4,1)");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "aaac aaac", "Pass. Group[0]=(8,1)");
    t.run_regex_trial("\\Bc", RIGHT_TO_LEFT, "aaa caaa", "Fail.");
    t.run_regex_trial(
        "b(a?)b",
        RIGHT_TO_LEFT,
        "aabababbaaababa",
        "Pass. Group[0]=(11,3) Group[1]=(12,1)",
    );
    t.run_regex_trial(
        "b{4}",
        RIGHT_TO_LEFT,
        "abbbbaabbbbaabbb",
        "Pass. Group[0]=(7,4)",
    );
    t.run_regex_trial(
        "b\\1aa(.)",
        RIGHT_TO_LEFT,
        "bBaaB",
        "Pass. Group[0]=(0,5) Group[1]=(4,1)",
    );
    t.run_regex_trial("b(.)aa\\1", RIGHT_TO_LEFT, "bBaaB", "Fail.");
    t.run_regex_trial(
        "^(a\\1?){4}$",
        RIGHT_TO_LEFT,
        "aaaaaa",
        "Pass. Group[0]=(0,6) Group[1]=(5,1)(3,2)(2,1)(0,2)",
    );
    t.run_regex_trial(
        "^([0-9a-fA-F]+)(?:x([0-9a-fA-F]+)?)(?:x([0-9a-fA-F]+))?",
        RIGHT_TO_LEFT,
        "012cxx0190",
        "Pass. Group[0]=(0,10) Group[1]=(0,4) Group[2]= Group[3]=(6,4)",
    );
    t.run_regex_trial(
        "^(b+?|a){1,2}c",
        RIGHT_TO_LEFT,
        "bbbac",
        "Pass. Group[0]=(0,5) Group[1]=(3,1)(0,3)",
    );
    t.run_regex_trial(
        "\\((\\w\\. \\w+)\\)",
        RIGHT_TO_LEFT,
        "cd. (A. Tw)",
        "Pass. Group[0]=(4,7) Group[1]=(5,5)",
    );
    t.run_regex_trial(
        "((?:aaaa|bbbb)cccc)?",
        RIGHT_TO_LEFT,
        "aaaacccc",
        "Pass. Group[0]=(0,8) Group[1]=(0,8)",
    );
    t.run_regex_trial(
        "((?:aaaa|bbbb)cccc)?",
        RIGHT_TO_LEFT,
        "bbbbcccc",
        "Pass. Group[0]=(0,8) Group[1]=(0,8)",
    );
    t.run_regex_trial("(?<=a)b", RIGHT_TO_LEFT, "ab", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("(?<=a)b", RIGHT_TO_LEFT, "cb", "Fail.");
    t.run_regex_trial("(?<=a)b", RIGHT_TO_LEFT, "b", "Fail.");
    t.run_regex_trial("(?<!c)b", RIGHT_TO_LEFT, "ab", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("(?<!c)b", RIGHT_TO_LEFT, "cb", "Fail.");
    t.run_regex_trial("(?<!c)b", RIGHT_TO_LEFT, "b", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("(?<!c)b", RIGHT_TO_LEFT, "b", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a(?=d).", RIGHT_TO_LEFT, "adabad", "Pass. Group[0]=(4,2)");
    t.run_regex_trial("a(?=c|d).", RIGHT_TO_LEFT, "adabad", "Pass. Group[0]=(4,2)");
    t.run_regex_trial("ab*c", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab*bc", RIGHT_TO_LEFT, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab*bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial(".{1}", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(5,1)");
    t.run_regex_trial(".{3,4}", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(2,4)");
    t.run_regex_trial("ab{0,}bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab+bc", RIGHT_TO_LEFT, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab+bc", RIGHT_TO_LEFT, "abc", "Fail.");
    t.run_regex_trial("ab+bc", RIGHT_TO_LEFT, "abq", "Fail.");
    t.run_regex_trial("ab{1,}bc", RIGHT_TO_LEFT, "abq", "Fail.");
    t.run_regex_trial("ab+bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,}bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{1,3}bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{3,4}bc", RIGHT_TO_LEFT, "abbbbc", "Pass. Group[0]=(0,6)");
    t.run_regex_trial("ab{4,5}bc", RIGHT_TO_LEFT, "abbbbc", "Fail.");
    t.run_regex_trial("ab?bc", RIGHT_TO_LEFT, "abbc", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("ab?bc", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}bc", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab?bc", RIGHT_TO_LEFT, "abbbbc", "Fail.");
    t.run_regex_trial("ab?c", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("ab{0,1}c", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", RIGHT_TO_LEFT, "abcc", "Fail.");
    t.run_regex_trial("^abc", RIGHT_TO_LEFT, "abcc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^abc$", RIGHT_TO_LEFT, "aabc", "Fail.");
    t.run_regex_trial("abc$", RIGHT_TO_LEFT, "aabc", "Pass. Group[0]=(1,3)");
    t.run_regex_trial("abc$", RIGHT_TO_LEFT, "aabcd", "Fail.");
    t.run_regex_trial("^", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,0)");
    t.run_regex_trial("$", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(3,0)");
    t.run_regex_trial("a.c", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.c", RIGHT_TO_LEFT, "axc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a.*c", RIGHT_TO_LEFT, "axyzc", "Pass. Group[0]=(0,5)");
    t.run_regex_trial("a.*c", RIGHT_TO_LEFT, "axyzd", "Fail.");
    t.run_regex_trial("a[bc]d", RIGHT_TO_LEFT, "abc", "Fail.");
    t.run_regex_trial("a[bc]d", RIGHT_TO_LEFT, "abd", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]e", RIGHT_TO_LEFT, "abd", "Fail.");
    t.run_regex_trial("a[b-d]e", RIGHT_TO_LEFT, "ace", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[b-d]", RIGHT_TO_LEFT, "aac", "Pass. Group[0]=(1,2)");
    t.run_regex_trial("a[-b]", RIGHT_TO_LEFT, "a-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-]", RIGHT_TO_LEFT, "a-", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[b-a]", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("a[]b", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("a[", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("a]", RIGHT_TO_LEFT, "a]", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a[]]b", RIGHT_TO_LEFT, "a]b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", RIGHT_TO_LEFT, "aed", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^bc]d", RIGHT_TO_LEFT, "abd", "Fail.");
    t.run_regex_trial("a[^-b]c", RIGHT_TO_LEFT, "adc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[^-b]c", RIGHT_TO_LEFT, "a-c", "Fail.");
    t.run_regex_trial("a[^]b]c", RIGHT_TO_LEFT, "a]c", "Fail.");
    t.run_regex_trial("a[^]b]c", RIGHT_TO_LEFT, "adc", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("\\ba\\b", RIGHT_TO_LEFT, "a-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\ba\\b", RIGHT_TO_LEFT, "-a", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\ba\\b", RIGHT_TO_LEFT, "-a-", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\by\\b", RIGHT_TO_LEFT, "xy", "Fail.");
    t.run_regex_trial("\\by\\b", RIGHT_TO_LEFT, "yz", "Fail.");
    t.run_regex_trial("\\by\\b", RIGHT_TO_LEFT, "xyz", "Fail.");
    t.run_regex_trial("\\Ba\\B", RIGHT_TO_LEFT, "a-", "Fail.");
    t.run_regex_trial("\\Ba\\B", RIGHT_TO_LEFT, "-a", "Fail.");
    t.run_regex_trial("\\Ba\\B", RIGHT_TO_LEFT, "-a-", "Fail.");
    t.run_regex_trial("\\By\\b", RIGHT_TO_LEFT, "xy", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\by\\B", RIGHT_TO_LEFT, "yz", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\By\\B", RIGHT_TO_LEFT, "xyz", "Pass. Group[0]=(1,1)");
    t.run_regex_trial("\\w", RIGHT_TO_LEFT, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\w", RIGHT_TO_LEFT, "-", "Fail.");
    t.run_regex_trial("\\W", RIGHT_TO_LEFT, "a", "Fail.");
    t.run_regex_trial("\\W", RIGHT_TO_LEFT, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a\\sb", RIGHT_TO_LEFT, "a b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a\\sb", RIGHT_TO_LEFT, "a-b", "Fail.");
    t.run_regex_trial("a\\Sb", RIGHT_TO_LEFT, "a b", "Fail.");
    t.run_regex_trial("a\\Sb", RIGHT_TO_LEFT, "a-b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("\\d", RIGHT_TO_LEFT, "1", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("\\d", RIGHT_TO_LEFT, "-", "Fail.");
    t.run_regex_trial("\\D", RIGHT_TO_LEFT, "1", "Fail.");
    t.run_regex_trial("\\D", RIGHT_TO_LEFT, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\w]", RIGHT_TO_LEFT, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\w]", RIGHT_TO_LEFT, "-", "Fail.");
    t.run_regex_trial("[\\W]", RIGHT_TO_LEFT, "a", "Fail.");
    t.run_regex_trial("[\\W]", RIGHT_TO_LEFT, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("a[\\s]b", RIGHT_TO_LEFT, "a b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a[\\s]b", RIGHT_TO_LEFT, "a-b", "Fail.");
    t.run_regex_trial("a[\\S]b", RIGHT_TO_LEFT, "a b", "Fail.");
    t.run_regex_trial("a[\\S]b", RIGHT_TO_LEFT, "a-b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("[\\d]", RIGHT_TO_LEFT, "1", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("[\\d]", RIGHT_TO_LEFT, "-", "Fail.");
    t.run_regex_trial("[\\D]", RIGHT_TO_LEFT, "1", "Fail.");
    t.run_regex_trial("[\\D]", RIGHT_TO_LEFT, "-", "Pass. Group[0]=(0,1)");
    t.run_regex_trial("ab|cd", RIGHT_TO_LEFT, "abc", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("ab|cd", RIGHT_TO_LEFT, "abcd", "Pass. Group[0]=(2,2)");
    t.run_regex_trial(
        "()ef",
        RIGHT_TO_LEFT,
        "def",
        "Pass. Group[0]=(1,2) Group[1]=(1,0)",
    );
    t.run_regex_trial("*a", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("(*)b", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("$b", RIGHT_TO_LEFT, "b", "Fail.");
    t.run_regex_trial("a\\", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("a\\(b", RIGHT_TO_LEFT, "a(b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("a\\(*b", RIGHT_TO_LEFT, "ab", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("a\\(*b", RIGHT_TO_LEFT, "a((b", "Pass. Group[0]=(0,4)");
    t.run_regex_trial("a\\\\b", RIGHT_TO_LEFT, "a\\b", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc)", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("(abc", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial(
        "((a))",
        RIGHT_TO_LEFT,
        "abc",
        "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1)",
    );
    t.run_regex_trial(
        "(a)b(c)",
        RIGHT_TO_LEFT,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(2,1)",
    );
    t.run_regex_trial("a+b+c", RIGHT_TO_LEFT, "aabbabc", "Pass. Group[0]=(4,3)");
    t.run_regex_trial(
        "a{1,}b{1,}c",
        RIGHT_TO_LEFT,
        "aabbabc",
        "Pass. Group[0]=(4,3)",
    );
    t.run_regex_trial("a**", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("a.+?c", RIGHT_TO_LEFT, "abcabc", "Pass. Group[0]=(3,3)");
    t.run_regex_trial(
        "(a+|b)*",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(1,1)(0,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,}",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(1,1)(0,1)",
    );
    t.run_regex_trial(
        "(a+|b)+",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(1,1)(0,1)",
    );
    t.run_regex_trial(
        "(a+|b){1,}",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(0,2) Group[1]=(1,1)(0,1)",
    );
    t.run_regex_trial(
        "(a+|b)?",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(1,1) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "(a+|b){0,1}",
        RIGHT_TO_LEFT,
        "ab",
        "Pass. Group[0]=(1,1) Group[1]=(1,1)",
    );
    t.run_regex_trial(")(", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("[^ab]*", RIGHT_TO_LEFT, "cde", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("abc", RIGHT_TO_LEFT, "", "Fail.");
    t.run_regex_trial("a*", RIGHT_TO_LEFT, "", "Pass. Group[0]=(0,0)");
    t.run_regex_trial(
        "([abc])*d",
        RIGHT_TO_LEFT,
        "abbbcd",
        "Pass. Group[0]=(0,6) Group[1]=(4,1)(3,1)(2,1)(1,1)(0,1)",
    );
    t.run_regex_trial(
        "([abc])*bcd",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(0,1)",
    );
    t.run_regex_trial("a|b|c|d|e", RIGHT_TO_LEFT, "e", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "(a|b|c|d|e)f",
        RIGHT_TO_LEFT,
        "ef",
        "Pass. Group[0]=(0,2) Group[1]=(0,1)",
    );
    t.run_regex_trial("abcd*efg", RIGHT_TO_LEFT, "abcdefg", "Pass. Group[0]=(0,7)");
    t.run_regex_trial("ab*", RIGHT_TO_LEFT, "xabyabbbz", "Pass. Group[0]=(4,4)");
    t.run_regex_trial("ab*", RIGHT_TO_LEFT, "xayabbbz", "Pass. Group[0]=(3,4)");
    t.run_regex_trial(
        "(ab|cd)e",
        RIGHT_TO_LEFT,
        "abcde",
        "Pass. Group[0]=(2,3) Group[1]=(2,2)",
    );
    t.run_regex_trial("[abhgefdc]ij", RIGHT_TO_LEFT, "hij", "Pass. Group[0]=(0,3)");
    t.run_regex_trial("^(ab|cd)e", RIGHT_TO_LEFT, "abcde", "Fail.");
    t.run_regex_trial(
        "(abc|)ef",
        RIGHT_TO_LEFT,
        "abcdef",
        "Pass. Group[0]=(4,2) Group[1]=(4,0)",
    );
    t.run_regex_trial(
        "(a|b)c*d",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(1,3) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "(ab|ab*)bc",
        RIGHT_TO_LEFT,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "a([bc]*)c*",
        RIGHT_TO_LEFT,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "a([bc]*)(c*d)",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,1) Group[2]=(2,2)",
    );
    t.run_regex_trial(
        "a([bc]+)(c*d)",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,1) Group[2]=(2,2)",
    );
    t.run_regex_trial(
        "a([bc]*)(c+d)",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,1) Group[2]=(2,2)",
    );
    t.run_regex_trial(
        "a[bcd]*dcdcde",
        RIGHT_TO_LEFT,
        "adcdcde",
        "Pass. Group[0]=(0,7)",
    );
    t.run_regex_trial("a[bcd]+dcdcde", RIGHT_TO_LEFT, "adcdcde", "Fail.");
    t.run_regex_trial(
        "(ab|a)b*c",
        RIGHT_TO_LEFT,
        "abc",
        "Pass. Group[0]=(0,3) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "((a)(b)c)(d)",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(0,3) Group[2]=(0,1) Group[3]=(1,1) Group[4]=(3,1)",
    );
    t.run_regex_trial(
        "[a-zA-Z_][a-zA-Z0-9_]*",
        RIGHT_TO_LEFT,
        "alpha",
        "Pass. Group[0]=(0,5)",
    );
    t.run_regex_trial(
        "^a(bc+|b[eh])g|.h$",
        RIGHT_TO_LEFT,
        "abh",
        "Pass. Group[0]=(1,2) Group[1]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        RIGHT_TO_LEFT,
        "effgz",
        "Pass. Group[0]=(0,5) Group[1]=(0,5) Group[2]=",
    );
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        RIGHT_TO_LEFT,
        "ij",
        "Pass. Group[0]=(0,2) Group[1]=(0,2) Group[2]=(1,1)",
    );
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", RIGHT_TO_LEFT, "effg", "Fail.");
    t.run_regex_trial("(bc+d$|ef*g.|h?i(j|k))", RIGHT_TO_LEFT, "bcdd", "Fail.");
    t.run_regex_trial(
        "(bc+d$|ef*g.|h?i(j|k))",
        RIGHT_TO_LEFT,
        "reffgz",
        "Pass. Group[0]=(1,5) Group[1]=(1,5) Group[2]=",
    );
    t.run_regex_trial("((((((((((a))))))))))", RIGHT_TO_LEFT, "a", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("((((((((((a))))))))))\\10", RIGHT_TO_LEFT, "aa", "Fail.");
    t.run_regex_trial("\\10((((((((((a))))))))))", RIGHT_TO_LEFT, "aa", "Pass. Group[0]=(0,2) Group[1]=(1,1) Group[2]=(1,1) Group[3]=(1,1) Group[4]=(1,1) Group[5]=(1,1) Group[6]=(1,1) Group[7]=(1,1) Group[8]=(1,1) Group[9]=(1,1) Group[10]=(1,1)");
    t.run_regex_trial("((((((((((a))))))))))!", RIGHT_TO_LEFT, "aa", "Fail.");
    t.run_regex_trial("((((((((((a))))))))))!", RIGHT_TO_LEFT, "a!", "Pass. Group[0]=(0,2) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1) Group[10]=(0,1)");
    t.run_regex_trial("(((((((((a)))))))))", RIGHT_TO_LEFT, "a", "Pass. Group[0]=(0,1) Group[1]=(0,1) Group[2]=(0,1) Group[3]=(0,1) Group[4]=(0,1) Group[5]=(0,1) Group[6]=(0,1) Group[7]=(0,1) Group[8]=(0,1) Group[9]=(0,1)");
    t.run_regex_trial("multiple words of text", RIGHT_TO_LEFT, "uh-uh", "Fail.");
    t.run_regex_trial(
        "multiple words",
        RIGHT_TO_LEFT,
        "multiple words, yeah",
        "Pass. Group[0]=(0,14)",
    );
    t.run_regex_trial(
        "(.*)c(.*)",
        RIGHT_TO_LEFT,
        "abcde",
        "Pass. Group[0]=(0,5) Group[1]=(0,2) Group[2]=(3,2)",
    );
    t.run_regex_trial(
        "\\((.*), (.*)\\)",
        RIGHT_TO_LEFT,
        "(a, b)",
        "Pass. Group[0]=(0,6) Group[1]=(1,1) Group[2]=(4,1)",
    );
    t.run_regex_trial("[k]", RIGHT_TO_LEFT, "ab", "Fail.");
    t.run_regex_trial("abcd", RIGHT_TO_LEFT, "abcd", "Pass. Group[0]=(0,4)");
    t.run_regex_trial(
        "a(bc)d",
        RIGHT_TO_LEFT,
        "abcd",
        "Pass. Group[0]=(0,4) Group[1]=(1,2)",
    );
    t.run_regex_trial("a[-]?c", RIGHT_TO_LEFT, "ac", "Pass. Group[0]=(0,2)");
    t.run_regex_trial("(abc)\\1", RIGHT_TO_LEFT, "abcabc", "Fail.");
    t.run_regex_trial(
        "\\1(abc)",
        RIGHT_TO_LEFT,
        "abcabc",
        "Pass. Group[0]=(0,6) Group[1]=(3,3)",
    );
    t.run_regex_trial("([a-c]*)\\1", RIGHT_TO_LEFT, "abcabc", "Fail.");
    t.run_regex_trial(
        "\\1([a-c]*)",
        RIGHT_TO_LEFT,
        "abcabc",
        "Pass. Group[0]=(0,6) Group[1]=(3,3)",
    );
    t.run_regex_trial("\\1", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial("\\2", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial(
        "(a)|\\1",
        RIGHT_TO_LEFT,
        "a",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial("(a)|\\1", RIGHT_TO_LEFT, "x", "Fail.");
    t.run_regex_trial("(a)|\\2", RIGHT_TO_LEFT, "-", "Error.");
    t.run_regex_trial(
        "(([a-c])b*?\\2)*",
        RIGHT_TO_LEFT,
        "ababbbcbc",
        "Pass. Group[0]=(9,0) Group[1]= Group[2]=",
    );
    t.run_regex_trial("(([a-c])b*?\\2){3}", RIGHT_TO_LEFT, "ababbbcbc", "Fail.");
    t.run_regex_trial("((\\3|b)\\2(a)x)+", RIGHT_TO_LEFT, "aaxabxbaxbbx", "Fail.");
    t.run_regex_trial(
        "((\\3|b)\\2(a)x)+",
        RIGHT_TO_LEFT,
        "aaaxabaxbaaxbbax",
        "Fail.",
    );
    t.run_regex_trial(
        "((\\3|b)\\2(a)){2,}",
        RIGHT_TO_LEFT,
        "bbaababbabaaaaabbaaaabba",
        "Fail.",
    );
    t.run_regex_trial(
        "\\((?>[^()]+|\\((?<depth>)|\\)(?<-depth>))*(?(depth)(?!))\\)",
        0,
        "((a(b))c)",
        "Pass. Group[0]=(0,9) Group[1]=",
    );
    t.run_regex_trial(
        "^\\((?>[^()]+|\\((?<depth>)|\\)(?<-depth>))*(?(depth)(?!))\\)$",
        0,
        "((a(b))c)",
        "Pass. Group[0]=(0,9) Group[1]=",
    );
    t.run_regex_trial(
        "^\\((?>[^()]+|\\((?<depth>)|\\)(?<-depth>))*(?(depth)(?!))\\)$",
        0,
        "((a(b))c",
        "Fail.",
    );
    t.run_regex_trial(
        "^\\((?>[^()]+|\\((?<depth>)|\\)(?<-depth>))*(?(depth)(?!))\\)$",
        0,
        "())",
        "Fail.",
    );
    t.run_regex_trial("(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))", 0, "((a(b))c)", "Pass. Group[0]=(0,9) Group[1]=(0,9) Group[2]=(0,1)(1,2)(3,2) Group[3]=(5,1)(6,2)(8,1) Group[4]= Group[5]=(4,1)(2,4)(1,7)");
    t.run_regex_trial("^(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))$", 0, "((a(b))c)", "Pass. Group[0]=(0,9) Group[1]=(0,9) Group[2]=(0,1)(1,2)(3,2) Group[3]=(5,1)(6,2)(8,1) Group[4]= Group[5]=(4,1)(2,4)(1,7)");
    t.run_regex_trial("(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))", 0, "x(a((b)))b)x", "Pass. Group[0]=(1,9) Group[1]=(1,9) Group[2]=(1,2)(3,1)(4,2) Group[3]=(6,1)(7,1)(8,2) Group[4]= Group[5]=(5,1)(4,3)(2,6)");
    t.run_regex_trial("(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))", 0, "x((a((b)))x", "Pass. Group[0]=(2,9) Group[1]=(2,9) Group[2]=(2,2)(4,1)(5,2) Group[3]=(7,1)(8,1)(9,2) Group[4]= Group[5]=(6,1)(5,3)(3,6)");
    t.run_regex_trial(
        "^(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))$",
        0,
        "((a(b))c",
        "Fail.",
    );
    t.run_regex_trial(
        "^(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))$",
        0,
        "((a(b))c))",
        "Fail.",
    );
    t.run_regex_trial(
        "^(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))$",
        0,
        ")(",
        "Fail.",
    );
    t.run_regex_trial(
        "^(((?<foo>\\()[^()]*)+((?<bar-foo>\\))[^()]*)+)+(?(foo)(?!))$",
        0,
        "((a((b))c)",
        "Fail.",
    );
    t.run_regex_trial("b", RIGHT_TO_LEFT, "babaaa", "Pass. Group[0]=(2,1)");
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "[n]",
        "Pass. Group[0]=(0,3) Group[1]=(1,1)",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "n",
        "Pass. Group[0]=(0,1) Group[1]=(0,1)",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "n[i]e",
        "Fail.",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "[n",
        "Fail.",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "]n]",
        "Fail.",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "\\[n\\]",
        "Fail.",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "[n\\]",
        "Pass. Group[0]=(0,4) Group[1]=(1,2)",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "[n\\[]",
        "Pass. Group[0]=(0,5) Group[1]=(1,3)",
    );
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE,
        "[[n]",
        "Pass. Group[0]=(0,4) Group[1]=(1,2)",
    );
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[s] . [n]", "Pass. Group[0]=(0,9) Group[1]=(1,1) Group[2]=(7,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[s] . n", "Pass. Group[0]=(0,7) Group[1]=(1,1) Group[2]=(6,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "s.[ n ]", "Pass. Group[0]=(0,7) Group[1]=(0,1) Group[2]=(3,3)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, " . n", "Pass. Group[0]=(0,4) Group[1]=(0,1) Group[2]=(3,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "s. ", "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(2,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[.]. ", "Pass. Group[0]=(0,5) Group[1]=(1,1) Group[2]=(4,1)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[c].[s].[n]", "Pass. Group[0]=(0,11) Group[1]=(1,1) Group[2]=(5,1) Group[3]=(9,1)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, " c . s . n ", "Pass. Group[0]=(0,11) Group[1]=(0,3) Group[2]=(5,2) Group[3]=(9,2)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, " . [.] . [ ]", "Pass. Group[0]=(0,12) Group[1]=(0,1) Group[2]=(4,1) Group[3]=(10,1)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "c.n", "Pass. Group[0]=(0,3) Group[1]=(0,1) Group[2]=(2,1)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[c] .[n]", "Pass. Group[0]=(0,8) Group[1]=(1,1) Group[2]=(6,1)");
    t.run_regex_trial("^((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "c.n.", "Fail.");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "s.c.n", "Pass. Group[0]=(0,5) Group[1]=(0,1) Group[2]=(2,1) Group[3]=(4,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[s].[c].[n]", "Pass. Group[0]=(0,11) Group[1]=(1,1) Group[2]=(5,1) Group[3]=(9,1)");
    t.run_regex_trial("^((\\[(?<SCHEMA>[^\\]]+)\\])|(?<SCHEMA>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<CATALOG>[^\\]]+)\\])|(?<CATALOG>[^\\.\\[\\]]+))\\s*\\.\\s*((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$", COMPILED | IGNORE_CASE | EXPLICIT_CAPTURE, "[s].[c].", "Fail.");
    t.run_regex_trial(
        "^((\\[(?<ColName>.+)\\])|(?<ColName>\\S+))([ ]+(?<Order>ASC|DESC))?$",
        IGNORE_CASE | EXPLICIT_CAPTURE,
        "[id]]",
        "Pass. Group[0]=(0,5) Group[1]=(1,3) Group[2]=",
    );
    t.run_regex_trial("a{1,2147483647}", 0, "a", "Pass. Group[0]=(0,1)");
    t.run_regex_trial(
        "^((\\[(?<NAME>[^\\]]+)\\])|(?<NAME>[^\\.\\[\\]]+))$",
        0,
        "[a]",
        "Pass. Group[0]=(0,3) Group[1]=(0,3) Group[2]=(0,3) Group[3]=(1,1)",
    );
    assert!(t.failures.is_empty(), "{}", t.failures.join("\n"));
}
