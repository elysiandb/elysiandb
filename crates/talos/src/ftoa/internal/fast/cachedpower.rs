use super::common::_dcheck;
use super::diyfp::{DIY_FP_K_SIGNIFICAND_SIZE, Diyfp};

const K_CACHED_POWERS_OFFSET: i64 = 348;
const K_D_1_LOG2_10: f64 = 0.30102999566398114;
const K_DECIMAL_EXPONENT_DISTANCE: i64 = 8;

struct CachedPower {
    significand: u64,
    binary_exponent: i16,
    decimal_exponent: i16,
}

static CACHED_POWERS: [CachedPower; 87] = [
    CachedPower {
        significand: 0xFA8F_D5A0_081C_0288,
        binary_exponent: -1220,
        decimal_exponent: -348,
    },
    CachedPower {
        significand: 0xBAAE_E17F_A23E_BF76,
        binary_exponent: -1193,
        decimal_exponent: -340,
    },
    CachedPower {
        significand: 0x8B16_FB20_3055_AC76,
        binary_exponent: -1166,
        decimal_exponent: -332,
    },
    CachedPower {
        significand: 0xCF42_894A_5DCE_35EA,
        binary_exponent: -1140,
        decimal_exponent: -324,
    },
    CachedPower {
        significand: 0x9A6B_B0AA_5565_3B2D,
        binary_exponent: -1113,
        decimal_exponent: -316,
    },
    CachedPower {
        significand: 0xE61A_CF03_3D1A_45DF,
        binary_exponent: -1087,
        decimal_exponent: -308,
    },
    CachedPower {
        significand: 0xAB70_FE17_C79A_C6CA,
        binary_exponent: -1060,
        decimal_exponent: -300,
    },
    CachedPower {
        significand: 0xFF77_B1FC_BEBC_DC4F,
        binary_exponent: -1034,
        decimal_exponent: -292,
    },
    CachedPower {
        significand: 0xBE56_91EF_416B_D60C,
        binary_exponent: -1007,
        decimal_exponent: -284,
    },
    CachedPower {
        significand: 0x8DD0_1FAD_907F_FC3C,
        binary_exponent: -980,
        decimal_exponent: -276,
    },
    CachedPower {
        significand: 0xD351_5C28_3155_9A83,
        binary_exponent: -954,
        decimal_exponent: -268,
    },
    CachedPower {
        significand: 0x9D71_AC8F_ADA6_C9B5,
        binary_exponent: -927,
        decimal_exponent: -260,
    },
    CachedPower {
        significand: 0xEA9C_2277_23EE_8BCB,
        binary_exponent: -901,
        decimal_exponent: -252,
    },
    CachedPower {
        significand: 0xAECC_4991_4078_536D,
        binary_exponent: -874,
        decimal_exponent: -244,
    },
    CachedPower {
        significand: 0x823C_1279_5DB6_CE57,
        binary_exponent: -847,
        decimal_exponent: -236,
    },
    CachedPower {
        significand: 0xC210_9436_4DFB_5637,
        binary_exponent: -821,
        decimal_exponent: -228,
    },
    CachedPower {
        significand: 0x9096_EA6F_3848_984F,
        binary_exponent: -794,
        decimal_exponent: -220,
    },
    CachedPower {
        significand: 0xD774_85CB_2582_3AC7,
        binary_exponent: -768,
        decimal_exponent: -212,
    },
    CachedPower {
        significand: 0xA086_CFCD_97BF_97F4,
        binary_exponent: -741,
        decimal_exponent: -204,
    },
    CachedPower {
        significand: 0xEF34_0A98_172A_ACE5,
        binary_exponent: -715,
        decimal_exponent: -196,
    },
    CachedPower {
        significand: 0xB238_67FB_2A35_B28E,
        binary_exponent: -688,
        decimal_exponent: -188,
    },
    CachedPower {
        significand: 0x84C8_D4DF_D2C6_3F3B,
        binary_exponent: -661,
        decimal_exponent: -180,
    },
    CachedPower {
        significand: 0xC5DD_4427_1AD3_CDBA,
        binary_exponent: -635,
        decimal_exponent: -172,
    },
    CachedPower {
        significand: 0x936B_9FCE_BB25_C996,
        binary_exponent: -608,
        decimal_exponent: -164,
    },
    CachedPower {
        significand: 0xDBAC_6C24_7D62_A584,
        binary_exponent: -582,
        decimal_exponent: -156,
    },
    CachedPower {
        significand: 0xA3AB_6658_0D5F_DAF6,
        binary_exponent: -555,
        decimal_exponent: -148,
    },
    CachedPower {
        significand: 0xF3E2_F893_DEC3_F126,
        binary_exponent: -529,
        decimal_exponent: -140,
    },
    CachedPower {
        significand: 0xB5B5_ADA8_AAFF_80B8,
        binary_exponent: -502,
        decimal_exponent: -132,
    },
    CachedPower {
        significand: 0x8762_5F05_6C7C_4A8B,
        binary_exponent: -475,
        decimal_exponent: -124,
    },
    CachedPower {
        significand: 0xC9BC_FF60_34C1_3053,
        binary_exponent: -449,
        decimal_exponent: -116,
    },
    CachedPower {
        significand: 0x964E_858C_91BA_2655,
        binary_exponent: -422,
        decimal_exponent: -108,
    },
    CachedPower {
        significand: 0xDFF9_7724_7029_7EBD,
        binary_exponent: -396,
        decimal_exponent: -100,
    },
    CachedPower {
        significand: 0xA6DF_BD9F_B8E5_B88F,
        binary_exponent: -369,
        decimal_exponent: -92,
    },
    CachedPower {
        significand: 0xF8A9_5FCF_8874_7D94,
        binary_exponent: -343,
        decimal_exponent: -84,
    },
    CachedPower {
        significand: 0xB944_7093_8FA8_9BCF,
        binary_exponent: -316,
        decimal_exponent: -76,
    },
    CachedPower {
        significand: 0x8A08_F0F8_BF0F_156B,
        binary_exponent: -289,
        decimal_exponent: -68,
    },
    CachedPower {
        significand: 0xCDB0_2555_6531_31B6,
        binary_exponent: -263,
        decimal_exponent: -60,
    },
    CachedPower {
        significand: 0x993F_E2C6_D07B_7FAC,
        binary_exponent: -236,
        decimal_exponent: -52,
    },
    CachedPower {
        significand: 0xE45C_10C4_2A2B_3B06,
        binary_exponent: -210,
        decimal_exponent: -44,
    },
    CachedPower {
        significand: 0xAA24_2499_6973_92D3,
        binary_exponent: -183,
        decimal_exponent: -36,
    },
    CachedPower {
        significand: 0xFD87_B5F2_8300_CA0E,
        binary_exponent: -157,
        decimal_exponent: -28,
    },
    CachedPower {
        significand: 0xBCE5_0864_9211_1AEB,
        binary_exponent: -130,
        decimal_exponent: -20,
    },
    CachedPower {
        significand: 0x8CBC_CC09_6F50_88CC,
        binary_exponent: -103,
        decimal_exponent: -12,
    },
    CachedPower {
        significand: 0xD1B7_1758_E219_652C,
        binary_exponent: -77,
        decimal_exponent: -4,
    },
    CachedPower {
        significand: 0x9C40_0000_0000_0000,
        binary_exponent: -50,
        decimal_exponent: 4,
    },
    CachedPower {
        significand: 0xE8D4_A510_0000_0000,
        binary_exponent: -24,
        decimal_exponent: 12,
    },
    CachedPower {
        significand: 0xAD78_EBC5_AC62_0000,
        binary_exponent: 3,
        decimal_exponent: 20,
    },
    CachedPower {
        significand: 0x813F_3978_F894_0984,
        binary_exponent: 30,
        decimal_exponent: 28,
    },
    CachedPower {
        significand: 0xC097_CE7B_C907_15B3,
        binary_exponent: 56,
        decimal_exponent: 36,
    },
    CachedPower {
        significand: 0x8F7E_32CE_7BEA_5C70,
        binary_exponent: 83,
        decimal_exponent: 44,
    },
    CachedPower {
        significand: 0xD5D2_38A4_ABE9_8068,
        binary_exponent: 109,
        decimal_exponent: 52,
    },
    CachedPower {
        significand: 0x9F4F_2726_179A_2245,
        binary_exponent: 136,
        decimal_exponent: 60,
    },
    CachedPower {
        significand: 0xED63_A231_D4C4_FB27,
        binary_exponent: 162,
        decimal_exponent: 68,
    },
    CachedPower {
        significand: 0xB0DE_6538_8CC8_ADA8,
        binary_exponent: 189,
        decimal_exponent: 76,
    },
    CachedPower {
        significand: 0x83C7_088E_1AAB_65DB,
        binary_exponent: 216,
        decimal_exponent: 84,
    },
    CachedPower {
        significand: 0xC45D_1DF9_4271_1D9A,
        binary_exponent: 242,
        decimal_exponent: 92,
    },
    CachedPower {
        significand: 0x924D_692C_A61B_E758,
        binary_exponent: 269,
        decimal_exponent: 100,
    },
    CachedPower {
        significand: 0xDA01_EE64_1A70_8DEA,
        binary_exponent: 295,
        decimal_exponent: 108,
    },
    CachedPower {
        significand: 0xA26D_A399_9AEF_774A,
        binary_exponent: 322,
        decimal_exponent: 116,
    },
    CachedPower {
        significand: 0xF209_787B_B47D_6B85,
        binary_exponent: 348,
        decimal_exponent: 124,
    },
    CachedPower {
        significand: 0xB454_E4A1_79DD_1877,
        binary_exponent: 375,
        decimal_exponent: 132,
    },
    CachedPower {
        significand: 0x865B_8692_5B9B_C5C2,
        binary_exponent: 402,
        decimal_exponent: 140,
    },
    CachedPower {
        significand: 0xC835_53C5_C896_5D3D,
        binary_exponent: 428,
        decimal_exponent: 148,
    },
    CachedPower {
        significand: 0x952A_B45C_FA97_A0B3,
        binary_exponent: 455,
        decimal_exponent: 156,
    },
    CachedPower {
        significand: 0xDE46_9FBD_99A0_5FE3,
        binary_exponent: 481,
        decimal_exponent: 164,
    },
    CachedPower {
        significand: 0xA59B_C234_DB39_8C25,
        binary_exponent: 508,
        decimal_exponent: 172,
    },
    CachedPower {
        significand: 0xF6C6_9A72_A398_9F5C,
        binary_exponent: 534,
        decimal_exponent: 180,
    },
    CachedPower {
        significand: 0xB7DC_BF53_54E9_BECE,
        binary_exponent: 561,
        decimal_exponent: 188,
    },
    CachedPower {
        significand: 0x88FC_F317_F222_41E2,
        binary_exponent: 588,
        decimal_exponent: 196,
    },
    CachedPower {
        significand: 0xCC20_CE9B_D35C_78A5,
        binary_exponent: 614,
        decimal_exponent: 204,
    },
    CachedPower {
        significand: 0x9816_5AF3_7B21_53DF,
        binary_exponent: 641,
        decimal_exponent: 212,
    },
    CachedPower {
        significand: 0xE2A0_B5DC_971F_303A,
        binary_exponent: 667,
        decimal_exponent: 220,
    },
    CachedPower {
        significand: 0xA8D9_D153_5CE3_B396,
        binary_exponent: 694,
        decimal_exponent: 228,
    },
    CachedPower {
        significand: 0xFB9B_7CD9_A4A7_443C,
        binary_exponent: 720,
        decimal_exponent: 236,
    },
    CachedPower {
        significand: 0xBB76_4C4C_A7A4_4410,
        binary_exponent: 747,
        decimal_exponent: 244,
    },
    CachedPower {
        significand: 0x8BAB_8EEF_B640_9C1A,
        binary_exponent: 774,
        decimal_exponent: 252,
    },
    CachedPower {
        significand: 0xD01F_EF10_A657_842C,
        binary_exponent: 800,
        decimal_exponent: 260,
    },
    CachedPower {
        significand: 0x9B10_A4E5_E991_3129,
        binary_exponent: 827,
        decimal_exponent: 268,
    },
    CachedPower {
        significand: 0xE710_9BFB_A19C_0C9D,
        binary_exponent: 853,
        decimal_exponent: 276,
    },
    CachedPower {
        significand: 0xAC28_20D9_623B_F429,
        binary_exponent: 880,
        decimal_exponent: 284,
    },
    CachedPower {
        significand: 0x8044_4B5E_7AA7_CF85,
        binary_exponent: 907,
        decimal_exponent: 292,
    },
    CachedPower {
        significand: 0xBF21_E440_03AC_DD2D,
        binary_exponent: 933,
        decimal_exponent: 300,
    },
    CachedPower {
        significand: 0x8E67_9C2F_5E44_FF8F,
        binary_exponent: 960,
        decimal_exponent: 308,
    },
    CachedPower {
        significand: 0xD433_179D_9C8C_B841,
        binary_exponent: 986,
        decimal_exponent: 316,
    },
    CachedPower {
        significand: 0x9E19_DB92_B4E3_1BA9,
        binary_exponent: 1013,
        decimal_exponent: 324,
    },
    CachedPower {
        significand: 0xEB96_BF6E_BADF_77D9,
        binary_exponent: 1039,
        decimal_exponent: 332,
    },
    CachedPower {
        significand: 0xAF87_023B_9BF0_EE6B,
        binary_exponent: 1066,
        decimal_exponent: 340,
    },
];

pub(super) fn get_cached_power_for_binary_exponent_range(
    min_exponent: i64,
    max_exponent: i64,
) -> (Diyfp, i64) {
    let k_q = DIY_FP_K_SIGNIFICAND_SIZE;
    let k = (((min_exponent + k_q - 1) as f64) * K_D_1_LOG2_10).ceil() as i64;
    let index = (K_CACHED_POWERS_OFFSET + k - 1) / K_DECIMAL_EXPONENT_DISTANCE + 1;
    let cached_power = &CACHED_POWERS[index as usize];
    _dcheck(min_exponent <= i64::from(cached_power.binary_exponent));
    _dcheck(i64::from(cached_power.binary_exponent) <= max_exponent);
    let decimal_exponent = i64::from(cached_power.decimal_exponent);
    let power = Diyfp {
        f: cached_power.significand,
        e: i64::from(cached_power.binary_exponent),
    };
    (power, decimal_exponent)
}
