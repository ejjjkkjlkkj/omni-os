#!/usr/bin/env python3
from __future__ import annotations

import unittest

from hda_route_graph import (
    GET_CONNECTION_LIST_ENTRY,
    SET_CONNECTION_SELECT,
    HdaRouteError,
    RawConnection,
    WIDGET_AUDIO_OUTPUT,
    WIDGET_MIXER,
    WIDGET_PIN,
    WIDGET_SELECTOR,
    build_route_plan,
    decode_connection_list,
    decode_raw_connection_entries,
    encode_set_connection_select,
    expand_connection_entries,
    find_output_route,
)


class ConnectionListTests(unittest.TestCase):
    def test_short_form_four_entries(self):
        self.assertEqual(
            decode_connection_list(0x04, {0: 0x05040302}),
            (0x02, 0x03, 0x04, 0x05),
        )

    def test_short_form_multiple_responses(self):
        self.assertEqual(
            decode_connection_list(0x05, {0: 0x05040302, 4: 0x00000006}),
            (0x02, 0x03, 0x04, 0x05, 0x06),
        )

    def test_long_form_two_entries(self):
        self.assertEqual(
            decode_connection_list(0x82, {0: (0x0103 << 16) | 0x0102}),
            (0x0102, 0x0103),
        )

    def test_short_range_expands(self):
        self.assertEqual(
            decode_connection_list(0x03, {0: 0x00209210}),
            (0x10, 0x11, 0x12, 0x20),
        )

    def test_long_range_expands(self):
        self.assertEqual(
            decode_connection_list(0x82, {0: (0x8104 << 16) | 0x0102}),
            (0x0102, 0x0103, 0x0104),
        )

    def test_first_range_rejected(self):
        with self.assertRaises(HdaRouteError):
            decode_raw_connection_entries(0x01, {0: 0x00000082})

    def test_consecutive_range_ends_rejected(self):
        with self.assertRaises(HdaRouteError):
            expand_connection_entries((
                RawConnection(0x10),
                RawConnection(0x12, True),
                RawConnection(0x14, True),
            ))

    def test_descending_range_rejected(self):
        with self.assertRaises(HdaRouteError):
            expand_connection_entries((
                RawConnection(0x12),
                RawConnection(0x10, True),
            ))

    def test_missing_response_rejected(self):
        with self.assertRaises(HdaRouteError):
            decode_connection_list(0x05, {0: 0x05040302})

    def test_zero_nid_rejected(self):
        with self.assertRaises(HdaRouteError):
            decode_connection_list(0x01, {0: 0})


class GraphRouteTests(unittest.TestCase):
    def setUp(self):
        self.types = {
            0x02: WIDGET_AUDIO_OUTPUT,
            0x03: WIDGET_AUDIO_OUTPUT,
            0x0B: WIDGET_SELECTOR,
            0x0C: WIDGET_MIXER,
            0x14: WIDGET_PIN,
        }

    def test_direct_route(self):
        self.assertEqual(
            find_output_route(0x14, self.types, {0x14: (0x02,)}),
            (0x14, 0x02),
        )

    def test_multihop_route(self):
        conns = {
            0x14: (0x0C,),
            0x0C: (0x0B,),
            0x0B: (0x03, 0x02),
        }
        plan = build_route_plan(0x14, self.types, conns)
        self.assertEqual(plan.path, (0x14, 0x0C, 0x0B, 0x03))
        self.assertEqual(plan.selectors, ((0x0B, 0),))

    def test_selector_index_for_second_dac(self):
        types = dict(self.types)
        types.pop(0x03)
        conns = {
            0x14: (0x0C,),
            0x0C: (0x0B,),
            0x0B: (0x03, 0x02),
        }
        plan = build_route_plan(0x14, types, conns)
        self.assertEqual(plan.path, (0x14, 0x0C, 0x0B, 0x02))
        self.assertEqual(plan.selectors, ((0x0B, 1),))

    def test_pin_and_nested_selector_writes(self):
        types = dict(self.types)
        types[0x15] = WIDGET_MIXER
        types.pop(0x03)
        conns = {
            0x14: (0x15, 0x0C),
            0x15: (),
            0x0C: (0x0B,),
            0x0B: (0x03, 0x02),
        }
        plan = build_route_plan(0x14, types, conns)
        self.assertEqual(plan.path, (0x14, 0x0C, 0x0B, 0x02))
        self.assertEqual(plan.selectors, ((0x14, 1), (0x0B, 1)))

    def test_mixer_is_not_programmed_as_selector(self):
        conns = {
            0x14: (0x0C,),
            0x0C: (0x02, 0x03),
        }
        plan = build_route_plan(0x14, self.types, conns)
        self.assertEqual(plan.path, (0x14, 0x0C, 0x02))
        self.assertEqual(plan.selectors, ())

    def test_cycle_is_bounded(self):
        conns = {
            0x14: (0x0C,),
            0x0C: (0x0B,),
            0x0B: (0x0C,),
        }
        with self.assertRaises(HdaRouteError):
            find_output_route(0x14, self.types, conns, max_depth=8)

    def test_no_route_rejected(self):
        with self.assertRaises(HdaRouteError):
            find_output_route(0x14, self.types, {0x14: (0x0C,), 0x0C: ()})

    def test_non_pin_start_rejected(self):
        with self.assertRaises(HdaRouteError):
            find_output_route(0x0C, self.types, {0x0C: (0x02,)})


class VerbEncodingTests(unittest.TestCase):
    def test_constants(self):
        self.assertEqual(GET_CONNECTION_LIST_ENTRY, 0xF02)
        self.assertEqual(SET_CONNECTION_SELECT, 0x701)

    def test_set_connection_select_encoding(self):
        self.assertEqual(
            encode_set_connection_select(2, 0x14, 3),
            0x21470103,
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
