-- Track the API endpoint path each inference job arrived through
-- e.g. "/v1/chat/completions", "/api/chat", "/v1beta/models/gemini-2.0-flash:generateContent"
ALTER TABLE inference_jobs
    ADD COLUMN request_path TEXT;
