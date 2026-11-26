pub const STREAMS: &[(&str, &str, &str)] = &[
    (
        "degenerate HCLenTree",
        "05e001000000000010000000000000000000000000000000000000000000000000000000000000000004",
        "fail",
    ),
    (
        "complete HCLenTree, empty HLitTree, empty HDistTree",
        "05e001040000000000000000000000000000000000000000000000000000000000000000000000000010",
        "fail",
    ),
    (
        "empty HCLenTree",
        "05e001000000000000000000000000000000000000000000000000000000000000000000000000000010",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree, empty HDistTree, use missing HDist symbol",
        "000100feff000de00104000000001000000000000000000000000000000000000000000000000000000000000000002c",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree, degenerate HDistTree, use missing HDist symbol",
        "000100feff000de001000000000000000000000000000000000000000000000000000000000000000610000000004070",
        "fail",
    ),
    (
        "complete HCLenTree, empty HLitTree, empty HDistTree",
        "05e00104000000001004000000000000000000000000000000000000000000000000000000000000000000000008",
        "fail",
    ),
    (
        "complete HCLenTree, empty HLitTree, degenerate HDistTree",
        "05e00104000000001004000000000000000000000000000000000000000000000000000000000000000800000008",
        "fail",
    ),
    (
        "complete HCLenTree, degenerate HLitTree, degenerate HDistTree, use missing HLit symbol",
        "05e00104000000001000000000000000000000000000000000000000000000000000000000000000001c",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree, too large HDistTree",
        "edff870500000000200400000000000000000000000000000000000000000000000000000000000000080000000000000004",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree, empty HDistTree, excessive repeater code",
        "edfd870500000000200400000000000000000000000000000000000000000000000000000000000000e8b100",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree, empty HDistTree of normal length 30",
        "05fd01240000000000f8ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff07000000fe01",
        "",
    ),
    (
        "complete HCLenTree, complete HLitTree, empty HDistTree of excessive length 31",
        "05fe01240000000000f8ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff07000000fc03",
        "fail",
    ),
    (
        "complete HCLenTree, over-subscribed HLitTree, empty HDistTree",
        "05e001240000000000fcffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff07f00f",
        "fail",
    ),
    (
        "complete HCLenTree, under-subscribed HLitTree, empty HDistTree",
        "05e001240000000000fcfffffffffffffffffffffffffffffffffffffffffffffffffffffcffffffff07f00f",
        "fail",
    ),
    (
        "complete HCLenTree, complete HLitTree with single code, empty HDistTree",
        "05e001240000000000f8ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff07f00f",
        "01",
    ),
    (
        "complete HCLenTree, complete HLitTree with multiple codes, empty HDistTree",
        "05e301240000000000f8ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff07807f",
        "01",
    ),
    (
        "complete HCLenTree, complete HLitTree, degenerate HDistTree, use valid HDist symbol",
        "000100feff000de00104000000001000000000000000000000000000000000000000000000000000000000000000003c",
        "00000000",
    ),
    (
        "complete HCLenTree, degenerate HLitTree, degenerate HDistTree",
        "05e00104000000001000000000000000000000000000000000000000000000000000000000000000000c",
        "",
    ),
    (
        "complete HCLenTree, degenerate HLitTree, empty HDistTree",
        "05e001040000000010000000000000000000000000000000000000000000000000000000000000000004",
        "",
    ),
    (
        "complete HCLenTree, complete HLitTree, empty HDistTree, spanning repeater code",
        "edfd870500000000200400000000000000000000000000000000000000000000000000000000000000e8b000",
        "",
    ),
    (
        "complete HCLenTree with length codes, complete HLitTree, empty HDistTree",
        "ede00104000000001000000000000000000000000000000000000000000000000000000000000000000400004000",
        "",
    ),
    (
        "complete HCLenTree, complete HLitTree, degenerate HDistTree, use valid HLit symbol 284 with count 31",
        "000100feff00ede0010400000000100000000000000000000000000000000000000000000000000000000000000000040000407f00",
        "00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    ),
    (
        "complete HCLenTree, complete HLitTree, degenerate HDistTree, use valid HLit and HDist symbols",
        "0cc2010d00000082b0ac4aff0eb07d27060000ffff",
        "616263616263",
    ),
    ("fixed block, use reserved symbol 287", "33180700", "fail"),
    ("raw block", "010100feff11", "11"),
    (
        "issue 10426 - over-subscribed HCLenTree causes a hang",
        "344c4a4e494d4b070000ff2e2eff2e2e2e2e2eff",
        "fail",
    ),
    (
        "issue 11030 - empty HDistTree unexpectedly leads to error",
        "05c0070600000080400fff37a0ca",
        "",
    ),
    (
        "issue 11033 - empty HDistTree unexpectedly leads to error",
        "050fb109c020cca5d017dcbca044881ee1034ec149c8980bbc413c2ab35be9dcb1473449922449922411202306ee97b0383a521b4ffdcf3217f9f7d3adb701",
        "3130303634342068652e706870005d05355f7ed957ff084a90925d19e3ebc6d0c6d7",
    ),
];
