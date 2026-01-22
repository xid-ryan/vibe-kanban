import React, { useState } from 'react';
import { useJwtAuth } from '@/contexts/AuthContext';

export const LoginPage: React.FC = () => {
  const [token, setToken] = useState('');
  const [error, setError] = useState('');
  const { setToken: saveToken } = useJwtAuth();

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();

    if (!token.trim()) {
      setError('Please enter a JWT token');
      return;
    }

    // Basic JWT validation (3 parts separated by dots)
    const parts = token.split('.');
    if (parts.length !== 3) {
      setError('Invalid JWT token format');
      return;
    }

    try {
      // Try to decode the payload to validate it's proper base64
      const payload = JSON.parse(atob(parts[1].replace(/-/g, '+').replace(/_/g, '/')));

      // Check if token is expired
      if (payload.exp && payload.exp * 1000 < Date.now()) {
        setError('Token has expired');
        return;
      }

      saveToken(token.trim());
      setError('');
    } catch (e) {
      setError('Invalid JWT token');
    }
  };

  const handlePasteExampleToken = (exampleToken: string) => {
    setToken(exampleToken);
    setError('');
  };

  return (
    <div className="new-design min-h-screen bg-primary flex items-center justify-center p-base">
      <div className="w-full max-w-md">
        <div className="bg-secondary rounded-lg border p-double">
          <h1 className="text-xl font-semibold text-high mb-base">
            Vibe Kanban - Login
          </h1>
          <p className="text-sm text-low mb-double">
            Enter your JWT token to access the multi-user mode
          </p>

          <form onSubmit={handleSubmit}>
            <div className="mb-base">
              <label htmlFor="token" className="block text-sm text-normal mb-half">
                JWT Token
              </label>
              <textarea
                id="token"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                placeholder="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."
                className="w-full px-base py-half bg-primary rounded border text-base text-normal placeholder:text-low focus:outline-none focus:ring-1 focus:ring-brand font-ibm-plex-mono"
                rows={4}
              />
            </div>

            {error && (
              <div className="mb-base px-base py-half bg-error/10 border border-error rounded text-sm text-error">
                {error}
              </div>
            )}

            <button
              type="submit"
              className="w-full px-base py-half bg-brand text-white rounded hover:bg-brand/90 text-base font-medium"
            >
              Login
            </button>
          </form>

          <div className="mt-double pt-double border-t">
            <p className="text-xs text-low mb-half">Test Tokens:</p>
            <div className="space-y-half">
              <button
                type="button"
                onClick={() =>
                  handlePasteExampleToken(
                    'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMTExMTExMS0xMTExLTExMTEtMTExMS0xMTExMTExMTExMTEiLCJlbWFpbCI6ImFsaWNlQGV4YW1wbGUuY29tIiwiaWF0IjoxNzY5MDg4MjE4LCJleHAiOjE3NjkxNzQ2MTh9.ccLeJWvRzWd9uubHDYJcS6xiGWPUnvfaBqDxwczAFfI'
                  )
                }
                className="w-full px-base py-half bg-primary rounded border text-xs text-normal hover:bg-secondary text-left"
              >
                <span className="font-medium">Alice</span>
                <span className="text-low ml-half">(alice@example.com)</span>
              </button>
              <button
                type="button"
                onClick={() =>
                  handlePasteExampleToken(
                    'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIyMjIyMjIyMi0yMjIyLTIyMjItMjIyMi0yMjIyMjIyMjIyMjIiLCJlbWFpbCI6ImJvYkBleGFtcGxlLmNvbSIsImlhdCI6MTc2OTA4ODIyNSwiZXhwIjoxNzY5MTc0NjI1fQ.4BJJaUahSm72PCGrSCGiBBq3d4_TtXaMDITacjN6DJY'
                  )
                }
                className="w-full px-base py-half bg-primary rounded border text-xs text-normal hover:bg-secondary text-left"
              >
                <span className="font-medium">Bob</span>
                <span className="text-low ml-half">(bob@example.com)</span>
              </button>
            </div>
          </div>
        </div>

        <div className="mt-base text-center">
          <p className="text-xs text-low">
            Generate tokens with: <code className="font-ibm-plex-mono">node scripts/generate-test-jwt.js</code>
          </p>
        </div>
      </div>
    </div>
  );
};
