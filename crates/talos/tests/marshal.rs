use gocompat::json::quote_text;
use gocompat::strconv::format_float;
use std::collections::BTreeMap;
use talos::ast::*;
use talos::parser::Parser;
use talos::unistring;

enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(Vec<u8>),
    Arr(Vec<J>),
    Obj(BTreeMap<String, J>),
}

fn obj(pairs: Vec<(&str, J)>) -> J {
    J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn named(name: &str, child: J) -> J {
    obj(vec![(name, child)])
}

fn uni(s: &unistring::String) -> J {
    J::Str(s.as_bytes().into_owned())
}

fn text(s: &str) -> J {
    J::Str(s.as_bytes().to_vec())
}

fn raw(s: &[u8]) -> J {
    J::Str(s.to_vec())
}

fn float_json(f: f64) -> String {
    let abs = f.abs();
    let mut fmt = b'f';
    if abs != 0.0 && !(1e-6..1e21).contains(&abs) {
        fmt = b'e';
    }
    let mut b = format_float(f, fmt, -1, 64).into_bytes();
    if fmt == b'e' {
        let n = b.len();
        if n >= 4 && b[n - 4] == b'e' && b[n - 3] == b'-' && b[n - 2] == b'0' {
            b[n - 2] = b[n - 1];
            b.truncate(n - 1);
        }
    }
    String::from_utf8(b).expect("ascii")
}

fn number(v: &NumberValue) -> J {
    match v {
        NumberValue::Int64(i) | NumberValue::Int(i) => J::Num(i.to_string()),
        NumberValue::Float64(f) => J::Num(float_json(*f)),
        NumberValue::BigInt(b) => J::Num(b.string()),
    }
}

fn raw_key(e: &Expression) -> J {
    match e {
        Expression::StringLiteral(s) => obj(vec![
            ("Idx", J::Num(s.idx.to_string())),
            ("Literal", raw(&s.literal)),
            ("Value", uni(&s.value)),
        ]),
        Expression::NumberLiteral(n) => obj(vec![
            ("Idx", J::Num(n.idx.to_string())),
            ("Literal", raw(&n.literal)),
            ("Value", number(&n.value)),
        ]),
        other => panic!("unsupported key {other:?}"),
    }
}

fn exprs(list: &[Expression]) -> J {
    J::Arr(list.iter().map(expr).collect())
}

fn opt_expr(e: &Option<Expression>) -> J {
    match e {
        Some(e) => expr(e),
        None => J::Null,
    }
}

fn stmts(list: &[Statement]) -> J {
    J::Arr(list.iter().map(stmt).collect())
}

fn binding(b: &Binding) -> J {
    named(
        "Binding",
        obj(vec![
            ("Target", target(&b.target)),
            ("Initializer", opt_expr(&b.initializer)),
        ]),
    )
}

fn target(t: &BindingTarget) -> J {
    match t {
        BindingTarget::Identifier(i) => named("Identifier", uni(&i.name)),
        _ => J::Null,
    }
}

fn property(p: &Property) -> J {
    match p {
        Property::PropertyKeyed(k) => {
            obj(vec![("Key", raw_key(&k.key)), ("Value", expr(&k.value))])
        }
        _ => J::Null,
    }
}

fn expr(e: &Expression) -> J {
    match e {
        Expression::ArrayLiteral(n) => {
            named("Array", J::Arr(n.value.iter().map(opt_expr).collect()))
        }
        Expression::AssignExpression(n) => named(
            "Assign",
            obj(vec![("Left", expr(&n.left)), ("Right", expr(&n.right))]),
        ),
        Expression::BinaryExpression(n) => named(
            "BinaryExpression",
            obj(vec![
                ("Operator", text(&n.operator.string())),
                ("Left", expr(&n.left)),
                ("Right", expr(&n.right)),
            ]),
        ),
        Expression::BooleanLiteral(n) => named("Literal", J::Bool(n.value)),
        Expression::CallExpression(n) => named(
            "Call",
            obj(vec![
                ("Callee", expr(&n.callee)),
                ("ArgumentList", exprs(&n.argument_list)),
            ]),
        ),
        Expression::ConditionalExpression(n) => named(
            "Conditional",
            obj(vec![
                ("Test", expr(&n.test)),
                ("Consequent", expr(&n.consequent)),
                ("Alternate", expr(&n.alternate)),
            ]),
        ),
        Expression::DotExpression(n) => named(
            "Dot",
            obj(vec![
                ("Left", expr(&n.left)),
                ("Member", uni(&n.identifier.name)),
            ]),
        ),
        Expression::NewExpression(n) => named(
            "New",
            obj(vec![
                ("Callee", expr(&n.callee)),
                ("ArgumentList", exprs(&n.argument_list)),
            ]),
        ),
        Expression::NullLiteral(_) => named("Literal", J::Null),
        Expression::NumberLiteral(n) => named("Literal", number(&n.value)),
        Expression::ObjectLiteral(n) => {
            named("Object", J::Arr(n.value.iter().map(property).collect()))
        }
        Expression::RegExpLiteral(n) => named("Literal", raw(&n.literal)),
        Expression::StringLiteral(n) => named("Literal", raw(&n.literal)),
        Expression::FunctionLiteral(n) => named("Function", block(&n.body)),
        Expression::Identifier(n) => named("Identifier", uni(&n.name)),
        Expression::SequenceExpression(n) => named("Sequence", exprs(&n.sequence)),
        _ => J::Null,
    }
}

fn block(b: &BlockStatement) -> J {
    named("BlockStatement", stmts(&b.list))
}

fn for_into(i: &ForInto) -> J {
    match i {
        ForInto::ForDeclaration(d) => named("For-Into-Decl", target(&d.target)),
        ForInto::ForIntoVar(v) => named("For-Into-Var", binding(&v.binding)),
        ForInto::ForIntoExpression(_) => J::Null,
    }
}

fn stmt(s: &Statement) -> J {
    match s {
        Statement::BlockStatement(b) => block(b),
        Statement::EmptyStatement(_) => text("EmptyStatement"),
        Statement::ExpressionStatement(e) => expr(&e.expression),
        Statement::ForInStatement(f) => named(
            "ForIn",
            obj(vec![
                ("Into", for_into(&f.into)),
                ("Source", expr(&f.source)),
                ("Body", stmt(&f.body)),
            ]),
        ),
        Statement::IfStatement(n) => {
            let mut pairs = vec![("Test", expr(&n.test)), ("Consequent", stmt(&n.consequent))];
            if let Some(a) = &n.alternate {
                pairs.push(("Alternate", stmt(a)));
            }
            named("If", obj(pairs))
        }
        Statement::LabelledStatement(n) => named(
            "Label",
            obj(vec![
                ("Name", uni(&n.label.name)),
                ("Statement", stmt(&n.statement)),
            ]),
        ),
        Statement::ReturnStatement(n) => named("Return", opt_expr(&n.argument)),
        Statement::ThrowStatement(n) => named("Throw", expr(&n.argument)),
        Statement::VariableStatement(n) => {
            named("Var", J::Arr(n.list.iter().map(|b| binding(b)).collect()))
        }
        _ => J::Null,
    }
}

fn encode(v: &J, out: &mut String) {
    match v {
        J::Null => out.push_str("null"),
        J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        J::Num(n) => out.push_str(n),
        J::Str(s) => {
            let q = quote_text(s);
            out.push_str(
                &q.replace('<', "\\u003c")
                    .replace('>', "\\u003e")
                    .replace('&', "\\u0026"),
            );
        }
        J::Arr(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                encode(x, out);
            }
            out.push(']');
        }
        J::Obj(m) => {
            out.push('{');
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&quote_text(k.as_bytes()));
                out.push(':');
                encode(x, out);
            }
            out.push('}');
        }
    }
}

fn compact(s: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for c in s.chars() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_ascii_whitespace() {
            out.push(c);
        }
    }
    out
}

fn split(input_output: &str) -> (&str, &str) {
    let mut pos = 0;
    for line in input_output.split_inclusive('\n') {
        let body = line.trim_end_matches('\n');
        if body.trim_start_matches([' ', '\t']) == "---" {
            return (&input_output[..pos], &input_output[pos + line.len()..]);
        }
        pos += line.len();
    }
    panic!("no separator in {input_output:?}");
}

fn test(input_output: &str) {
    let (input, want) = split(input_output);
    let input = input.trim();
    let want = want.trim();
    let mut parser = Parser::new("", input.as_bytes());
    let (program, err) = parser.parse();
    assert!(err.is_none(), "{input}: {err:?}");
    let mut have = String::new();
    encode(&stmts(&program.body), &mut have);
    assert_eq!(have, compact(want), "{input}");
}

#[test]
fn test_parser_ast() {
    test(
        r##"
        ---
[]
        "##,
    );
    test(
        r##"
        ;
        ---
[
  "EmptyStatement"
]
        "##,
    );
    test(
        r##"
        ;;;
        ---
[
  "EmptyStatement",
  "EmptyStatement",
  "EmptyStatement"
]
        "##,
    );
    test(
        r##"
        1; true; abc; "abc"; null;
        ---
[
  {
    "Literal": 1
  },
  {
    "Literal": true
  },
  {
    "Identifier": "abc"
  },
  {
    "Literal": "\"abc\""
  },
  {
    "Literal": null
  }
]
        "##,
    );
    test(
        r##"
        { 1; null; 3.14159; ; }
        ---
[
  {
    "BlockStatement": [
      {
        "Literal": 1
      },
      {
        "Literal": null
      },
      {
        "Literal": 3.14159
      },
      "EmptyStatement"
    ]
  }
]
        "##,
    );
    test(
        r##"
        new abc();
        ---
[
  {
    "New": {
      "ArgumentList": [],
      "Callee": {
        "Identifier": "abc"
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        new abc(1, 3.14159)
        ---
[
  {
    "New": {
      "ArgumentList": [
        {
          "Literal": 1
        },
        {
          "Literal": 3.14159
        }
      ],
      "Callee": {
        "Identifier": "abc"
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        true ? false : true
        ---
[
  {
    "Conditional": {
      "Alternate": {
        "Literal": true
      },
      "Consequent": {
        "Literal": false
      },
      "Test": {
        "Literal": true
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        true || false
        ---
[
  {
    "BinaryExpression": {
      "Left": {
        "Literal": true
      },
      "Operator": "||",
      "Right": {
        "Literal": false
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        0 + { abc: true }
        ---
[
  {
    "BinaryExpression": {
      "Left": {
        "Literal": 0
      },
      "Operator": "+",
      "Right": {
        "Object": [
          {
            "Key": {
               "Idx": 7,
               "Literal": "abc",
               "Value": "abc"
            },
			"Value": {
              "Literal": true
            }
          }
        ]
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        1 == "1"
        ---
[
  {
    "BinaryExpression": {
      "Left": {
        "Literal": 1
      },
      "Operator": "==",
      "Right": {
        "Literal": "\"1\""
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        abc(1)
        ---
[
  {
    "Call": {
      "ArgumentList": [
        {
          "Literal": 1
        }
      ],
      "Callee": {
        "Identifier": "abc"
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        Math.pow(3, 2)
        ---
[
  {
    "Call": {
      "ArgumentList": [
        {
          "Literal": 3
        },
        {
          "Literal": 2
        }
      ],
      "Callee": {
        "Dot": {
          "Left": {
            "Identifier": "Math"
          },
          "Member": "pow"
        }
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        1, 2, 3
        ---
[
  {
    "Sequence": [
      {
        "Literal": 1
      },
      {
        "Literal": 2
      },
      {
        "Literal": 3
      }
    ]
  }
]
        "##,
    );
    test(
        r##"
        / abc /gim;
        ---
[
  {
    "Literal": "/ abc /gim"
  }
]
        "##,
    );
    test(
        r##"
        if (0)
            1;
        ---
[
  {
    "If": {
      "Consequent": {
        "Literal": 1
      },
      "Test": {
        "Literal": 0
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        0+function(){
            return;
        }
        ---
[
  {
    "BinaryExpression": {
      "Left": {
        "Literal": 0
      },
      "Operator": "+",
      "Right": {
        "Function": {
          "BlockStatement": [
            {
              "Return": null
            }
          ]
        }
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        xyzzy // Ignore it
        // Ignore this
        // And this
        /* And all..



        ... of this!
        */
        "Nothing happens."
        // And finally this
        ---
[
  {
    "Identifier": "xyzzy"
  },
  {
    "Literal": "\"Nothing happens.\""
  }
]
        "##,
    );
    test(
        r##"
        ((x & (x = 1)) !== 0)
        ---
[
  {
    "BinaryExpression": {
      "Left": {
        "BinaryExpression": {
          "Left": {
            "Identifier": "x"
          },
          "Operator": "\u0026",
          "Right": {
            "Assign": {
              "Left": {
                "Identifier": "x"
              },
              "Right": {
                "Literal": 1
              }
            }
          }
        }
      },
      "Operator": "!==",
      "Right": {
        "Literal": 0
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        { abc: 'def' }
        ---
[
  {
    "BlockStatement": [
      {
        "Label": {
          "Name": "abc",
          "Statement": {
            "Literal": "'def'"
          }
        }
      }
    ]
  }
]
        "##,
    );
    test(
        r##"
        // This is not an object, this is a string literal with a label!
        ({ abc: 'def' })
        ---
[
  {
    "Object": [
      {
		"Key": {
		   "Idx": 77,
		   "Literal": "abc",
		   "Value": "abc"
		},
		"Value": {
          "Literal": "'def'"
        }
      }
    ]
  }
]
        "##,
    );
    test(
        r##"
        [,]
        ---
[
  {
    "Array": [
      null
    ]
  }
]
        "##,
    );
    test(
        r##"
        [,,]
        ---
[
  {
    "Array": [
      null,
      null
    ]
  }
]
        "##,
    );
    test(
        r##"
        ({ get abc() {} })
        ---
[
  {
    "Object": [
      {
		"Key": {
		   "Idx": 8,
		   "Literal": "abc",
		   "Value": "abc"
		},
        "Value": {
          "Function": {
            "BlockStatement": []
          }
        }
      }
    ]
  }
]
        "##,
    );
    test(
        r##"
        /abc/.source
        ---
[
  {
    "Dot": {
      "Left": {
        "Literal": "/abc/"
      },
      "Member": "source"
    }
  }
]
        "##,
    );
    test(
        r##"
                xyzzy

        throw new TypeError("Nothing happens.")
        ---
[
  {
    "Identifier": "xyzzy"
  },
  {
    "Throw": {
      "New": {
        "ArgumentList": [
          {
            "Literal": "\"Nothing happens.\""
          }
        ],
        "Callee": {
          "Identifier": "TypeError"
        }
      }
    }
  }
]
	"##,
    );
    test(
        r##"
        var abc = 1
        (function(){
        })()
        ---
[
   {
      "Var": [
         {
            "Binding": {
               "Initializer": {
                  "Call": {
                     "ArgumentList": [],
                     "Callee": {
                        "Call": {
                           "ArgumentList": [
                              {
                                 "Function": {
                                    "BlockStatement": []
                                 }
                              }
                           ],
                           "Callee": {
                              "Literal": 1
                           }
                        }
                     }
                  }
               },
               "Target": {
                  "Identifier": "abc"
               }
            }
         }
      ]
   }
]
        "##,
    );
    test(
        r##"
        "use strict"
        ---
[
  {
    "Literal": "\"use strict\""
  }
]
        "##,
    );
    test(
        r##"
        "use strict"
        abc = 1 + 2 + 11
        ---
[
  {
    "Literal": "\"use strict\""
  },
  {
    "Assign": {
      "Left": {
        "Identifier": "abc"
      },
      "Right": {
        "BinaryExpression": {
          "Left": {
            "BinaryExpression": {
              "Left": {
                "Literal": 1
              },
              "Operator": "+",
              "Right": {
                "Literal": 2
              }
            }
          },
          "Operator": "+",
          "Right": {
            "Literal": 11
          }
        }
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        abc = function() { 'use strict' }
        ---
[
  {
    "Assign": {
      "Left": {
        "Identifier": "abc"
      },
      "Right": {
        "Function": {
          "BlockStatement": [
            {
              "Literal": "'use strict'"
            }
          ]
        }
      }
    }
  }
]
        "##,
    );
    test(
        r##"
        for (var abc in def) {
        }
        ---
[
   {
	  "ForIn": {
		 "Body": {
			"BlockStatement": []
		 },
		 "Into": {
			"For-Into-Var": {
			   "Binding": {
				  "Initializer": null,
				  "Target": {
					 "Identifier": "abc"
				  }
			   }
			}
		 },
		 "Source": {
			"Identifier": "def"
		 }
	  }
   }
]
        "##,
    );
    test(
        r##"
        abc = {
            '"': "'",
            "'": '"',
        }
        ---
[
  {
    "Assign": {
      "Left": {
        "Identifier": "abc"
      },
      "Right": {
        "Object": [
          {
			"Key": {
			   "Idx": 21,
               "Literal": "'\"'",
               "Value": "\""
            },
            "Value": {
              "Literal": "\"'\""
            }
          },
          {
			"Key": {
			   "Idx": 43,
			   "Literal": "\"'\"",
			   "Value": "'"
			},
            "Value": {
              "Literal": "'\"'"
            }
          }
        ]
      }
    }
  }
]
            "##,
    );
}
