"""Validation tests for public configuration settings."""
import unittest

from config import ObfuscationConfig


class ConfigTests(unittest.TestCase):
    def test_valid_boundary_values(self):
        config = ObfuscationConfig(
            name_min_length=1,
            name_max_length=1,
            mba_depth=3,
            min_blocks_for_cff=0,
            fake_function_count=0,
            seed=0,
        )
        self.assertEqual(config.seed, 0)

    def test_vm_profiles_enable_packing_only_when_hardened(self):
        self.assertFalse(ObfuscationConfig(virtualize=True, vm_hardening='basic').vm_pack_bytecode)
        self.assertTrue(ObfuscationConfig(virtualize=True, vm_hardening='max').vm_pack_bytecode)
        self.assertFalse(ObfuscationConfig(virtualize=True, vm_hardening='basic').vm_pack_constants)
        self.assertFalse(ObfuscationConfig(virtualize=True, vm_hardening='max').vm_pack_constants)
        self.assertFalse(ObfuscationConfig(virtualize=True, vm_hardening='basic').vm_whiten_constants)
        self.assertTrue(ObfuscationConfig(virtualize=True, vm_hardening='max').vm_whiten_constants)
        self.assertFalse(ObfuscationConfig(
            virtualize=True, vm_hardening='max', vm_pack_bytecode=False,
        ).vm_pack_bytecode)

    def test_invalid_ranges_are_rejected_early(self):
        cases = [
            {'name_min_length': 0},
            {'name_min_length': 5, 'name_max_length': 4},
            {'mba_depth': 0},
            {'mba_depth': 4},
            {'min_blocks_for_cff': -1},
            {'fake_function_count': -1},
            {'string_decode_mode': 'sometimes'},
        ]
        for kwargs in cases:
            with self.subTest(kwargs=kwargs), self.assertRaises(ValueError):
                ObfuscationConfig(**kwargs)

    def test_invalid_types_are_rejected_early(self):
        cases = [
            {'level': 3},
            {'dead_code_density': 'high'},
            {'name_min_length': 1.5},
            {'mba_depth': True},
            {'seed': '12345'},
        ]
        for kwargs in cases:
            with self.subTest(kwargs=kwargs), self.assertRaises(TypeError):
                ObfuscationConfig(**kwargs)


if __name__ == '__main__':
    unittest.main()
