import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("import_media", Path(__file__).with_name("import-media.py"))
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)


class MediaImportTests(unittest.TestCase):
    def test_movies_and_subtitles_are_hardlinked_without_removing_seed_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "completed" / "torrent"
            source.mkdir(parents=True)
            video = source / "Example.Movie.2020.mkv"
            subtitle = source / "Example.Movie.2020.en.srt"
            video.write_bytes(b"video")
            subtitle.write_text("subtitles")
            imported = hook.import_media(source, root / "movies", root / "tv", "Example Movie (2020)")
            target = root / "movies" / "Example Movie (2020)" / video.name
            self.assertIn(target, imported)
            self.assertTrue(video.samefile(target))
            self.assertTrue(subtitle.samefile(target.with_suffix(".en.srt")))
            self.assertTrue(video.exists())
            hook.import_media(source, root / "movies", root / "tv", "Example Movie (2020)")

    def test_tv_uses_existing_show_year_and_season_folders(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "completed"
            source.mkdir()
            video = source / "Louie.S02E03.720p.mp4"
            video.write_bytes(b"episode")
            (root / "tv" / "Louie (2010)").mkdir(parents=True)
            imported = hook.import_media(source, root / "movies", root / "tv", "Louie Season 2")
            target = root / "tv" / "Louie (2010)" / "Season 02" / video.name
            self.assertIn(target, imported)
            self.assertTrue(target.samefile(video))
            self.assertFalse((root / "movies").exists())

    def test_collision_never_replaces_library_files(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "completed"
            source.mkdir()
            (source / "movie.mp4").write_bytes(b"new")
            target = root / "movies" / "Movie" / "movie.mp4"
            target.parent.mkdir(parents=True)
            target.write_bytes(b"existing")
            with self.assertRaises(FileExistsError):
                hook.import_media(source, root / "movies", root / "tv", "Movie")
            self.assertEqual(target.read_bytes(), b"existing")

    def test_non_media_downloads_are_left_alone(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "completed"
            source.mkdir()
            (source / "linux.iso").write_bytes(b"iso")
            self.assertEqual(hook.import_media(source, root / "movies", root / "tv", "Linux"), [])
            self.assertFalse((root / "movies").exists())


if __name__ == "__main__":
    unittest.main()
