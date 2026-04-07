/**
 * Authentication middleware
 *
 * Verifies JWT tokens from Authorization header.
 * Used by protected endpoints (submit, rate, etc.)
 */

import crypto from 'crypto';

const JWT_SECRET = process.env.JWT_SECRET || 'dev-secret-change-in-production';

/**
 * Simple JWT verification (matches ato-cloud auth service)
 */
function verifyJwt(token) {
  try {
    const [headerB64, payloadB64, signatureB64] = token.split('.');
    if (!headerB64 || !payloadB64 || !signatureB64) {
      return null;
    }

    // Verify signature
    const data = `${headerB64}.${payloadB64}`;
    const expectedSig = crypto
      .createHmac('sha256', JWT_SECRET)
      .update(data)
      .digest('base64url');

    if (signatureB64 !== expectedSig) {
      return null;
    }

    // Decode payload
    const payload = JSON.parse(Buffer.from(payloadB64, 'base64url').toString());

    // Check expiration
    if (payload.exp && Date.now() >= payload.exp * 1000) {
      return null;
    }

    return payload;
  } catch (err) {
    return null;
  }
}

/**
 * Require authentication
 */
export function requireAuth(req, res, next) {
  const authHeader = req.headers.authorization;
  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    return res.status(401).json({ error: { message: 'Authentication required' } });
  }

  const token = authHeader.substring(7);
  const payload = verifyJwt(token);

  if (!payload || !payload.userId) {
    return res.status(401).json({ error: { message: 'Invalid or expired token' } });
  }

  req.user = {
    id: payload.userId,
    email: payload.email,
    name: payload.name,
  };

  next();
}

/**
 * Optional authentication (populates req.user if token present)
 */
export function optionalAuth(req, res, next) {
  const authHeader = req.headers.authorization;
  if (authHeader && authHeader.startsWith('Bearer ')) {
    const token = authHeader.substring(7);
    const payload = verifyJwt(token);
    if (payload && payload.userId) {
      req.user = {
        id: payload.userId,
        email: payload.email,
        name: payload.name,
      };
    }
  }
  next();
}

export default { requireAuth, optionalAuth };
