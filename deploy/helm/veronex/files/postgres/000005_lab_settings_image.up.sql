ALTER TABLE lab_settings
    ADD COLUMN max_images_per_request INTEGER NOT NULL DEFAULT 4,
    ADD COLUMN max_image_b64_bytes    INTEGER NOT NULL DEFAULT 2097152;
