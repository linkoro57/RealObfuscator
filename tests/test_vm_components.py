"""Focused tests for VM encoding components."""
import random
import unittest

from layers.vm.constant_pool import ConstantPool


class ConstantPoolTests(unittest.TestCase):
    def test_whitening_is_position_and_constant_specific(self):
        first = ConstantPool(random.Random(12345), whiten=True)
        second = ConstantPool(random.Random(12345), whiten=True)

        self.assertEqual(first.encrypt_bytes('same value', 1), second.encrypt_bytes('same value', 1))
        self.assertNotEqual(first.encrypt_bytes('same value', 1), first.encrypt_bytes('same value', 2))

    def test_whitening_stream_depends_on_value_length(self):
        pool = ConstantPool(random.Random(12345), whiten=True)
        short = pool.encrypt_bytes('repeated', 3)
        extended = pool.encrypt_bytes('repeated suffix', 3)
        self.assertNotEqual(short, extended[:len(short)])

    def test_basic_profile_keeps_legacy_stream(self):
        pool = ConstantPool(random.Random(12345), whiten=False)
        raw = 'value'.encode('utf-8')
        expected = bytes(byte ^ pool.key[i % len(pool.key)] for i, byte in enumerate(raw))
        self.assertEqual(pool.encrypt_bytes('value', 7), expected)


if __name__ == '__main__':
    unittest.main()
