"""Tests of Reel In's own logic, in the standard library: python -I -m unittest test_main.py.

None of them downloads anything or needs yt-dlp: main.py imports it only
when it runs.
"""

import unittest

import main


class Formats(unittest.TestCase):
    def test_best_takes_the_best_there_is(self):
        self.assertEqual(main.format_for("best"), "bv*+ba/b")

    def test_a_height_caps_both_ways_and_falls_back(self):
        self.assertEqual(main.format_for("1080"), "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba/b")
        self.assertEqual(main.format_for("2160"), "bv*[height<=2160]+ba/b[height<=2160]/bv*+ba/b")

    def test_an_unknown_quality_is_best(self):
        self.assertEqual(main.format_for("8k"), "bv*+ba/b")

    def test_the_settings_options_are_the_qualities(self):
        import json
        from pathlib import Path

        manifest = json.loads((Path(__file__).parent / "plugin.json").read_text(encoding="utf-8"))
        setting = manifest["contributes"]["settings"][0]
        self.assertEqual([o["value"] for o in setting["options"]], list(main.QUALITIES))


class Names(unittest.TestCase):
    def test_the_id_is_read_back_from_a_name(self):
        self.assertEqual(main.id_of(r"D:\Downloads\Show\Sprite Fright [_cMxraX_5RE].mkv"), "_cMxraX_5RE")
        self.assertEqual(main.id_of("/d/E003 - A [b] title [7329641234567890123].mp4"), "7329641234567890123")
        self.assertIsNone(main.id_of("/d/no id.mkv"))

    def test_a_playlist_video_is_numbered(self):
        self.assertTrue(main.NAME.startswith("%(playlist_index&E{:03d} - |)s"))
        self.assertIn("[%(id)s]", main.NAME)


class Errors(unittest.TestCase):
    def test_what_yt_dlp_says_becomes_a_plain_sentence(self):
        cases = {
            "ERROR: [youtube] abc: Private video. Sign in if you've been granted access": "This video is private.",
            "ERROR: [youtube] abc: Sign in to confirm your age.": "YouTube wants you signed in",
            "ERROR: [youtube] abc: Sign in to confirm you’re not a bot.": "not a bot",
            "ERROR: [youtube] abc: Video unavailable": "isn't there any more",
            "ERROR: Unsupported URL: https://example.com/x": "can't download from that link",
            "ERROR: unable to download video data: HTTP Error 429: Too Many Requests": "slow down",
            "ERROR: [Errno 28] No space left on device": "The drive is full.",
            "ERROR: abc: does not pass filter (!is_live & live_status!=is_upcoming), skipping": "Live streams",
            "ERROR: something else": "The download failed.",
        }
        for said, plain in cases.items():
            self.assertIn(plain, main.plain_error(said), said)


class Live(unittest.TestCase):
    def test_a_video_that_doesnt_say_whether_it_is_live_passes(self):
        # yt-dlp lets a missing field through only with "?".
        self.assertIn("live_status!=?is_upcoming", main.LIVE_FILTER)


class Progress(unittest.TestCase):
    def test_a_single_file(self):
        self.assertEqual(main.fraction(50, 100, 1, 1, 1, 1), 0.5)
        self.assertIsNone(main.fraction(50, None, 1, 1, 1, 1))

    def test_picture_and_sound(self):
        self.assertAlmostEqual(main.fraction(100, 100, 1, 2, 1, 1), 0.9)
        self.assertAlmostEqual(main.fraction(50, 100, 2, 2, 1, 1), 0.95)

    def test_a_playlist(self):
        self.assertAlmostEqual(main.fraction(50, 100, 1, 1, 2, 4), 0.375)
        self.assertLessEqual(main.fraction(200, 100, 1, 1, 9, 4), 1.0)

    def test_the_stage(self):
        self.assertEqual(main.stage("Sprite Fright", 1080), "“Sprite Fright” · 1080p")
        self.assertEqual(main.stage(None, None), "A video")


if __name__ == "__main__":
    unittest.main()
