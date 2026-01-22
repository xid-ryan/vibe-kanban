import React, { createContext, useContext, useState, useEffect, ReactNode } from 'react';

interface AuthContextType {
  token: string | null;
  setToken: (token: string | null) => void;
  isAuthenticated: boolean;
  logout: () => void;
}

const AuthContext = createContext<AuthContextType | undefined>(undefined);

const TOKEN_KEY = 'vibe_kanban_jwt_token';

export const AuthProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [token, setTokenState] = useState<string | null>(() => {
    // Initialize from localStorage
    try {
      return localStorage.getItem(TOKEN_KEY);
    } catch (e) {
      console.error('Failed to load token from localStorage:', e);
      return null;
    }
  });

  const setToken = (newToken: string | null) => {
    setTokenState(newToken);
    try {
      if (newToken) {
        localStorage.setItem(TOKEN_KEY, newToken);
      } else {
        localStorage.removeItem(TOKEN_KEY);
      }
    } catch (e) {
      console.error('Failed to save token to localStorage:', e);
    }
  };

  const logout = () => {
    setToken(null);
  };

  const value = {
    token,
    setToken,
    isAuthenticated: !!token,
    logout,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
};

export const useJwtAuth = (): AuthContextType => {
  const context = useContext(AuthContext);
  if (!context) {
    throw new Error('useJwtAuth must be used within an AuthProvider');
  }
  return context;
};
