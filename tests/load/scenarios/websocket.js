/**
 * WebSocket/Terminal Latency Test
 *
 * Tests PTY terminal WebSocket connections for latency requirements.
 * Validates that terminal input latency stays under 50ms.
 *
 * Note: This test requires WebSocket support and may need adjustments
 * based on the actual terminal WebSocket protocol implementation.
 *
 * Usage:
 *   k6 run scenarios/websocket.js
 *
 * Expected Results:
 *   - WebSocket connection time < 100ms
 *   - Message round-trip latency < 50ms at p95
 *   - Connection stability under load
 */

import { check, sleep } from 'k6';
import { Trend, Rate, Counter } from 'k6/metrics';
import ws from 'k6/ws';
import {
  BASE_URL,
  SMOKE_TEST_STAGES,
  THRESHOLDS,
} from '../config.js';
import { generateToken } from '../utils/jwt.js';

// Custom metrics
const wsConnectionTime = new Trend('ws_connection_time', true);
const wsMessageLatency = new Trend('ws_message_latency', true);
const wsErrors = new Rate('ws_errors');
const wsConnectionCount = new Counter('ws_connections');
const wsMessagesSent = new Counter('ws_messages_sent');
const wsMessagesReceived = new Counter('ws_messages_received');

// WebSocket URL (convert http to ws)
const WS_BASE_URL = BASE_URL.replace('http://', 'ws://').replace('https://', 'wss://');
const TERMINAL_WS_URL = `${WS_BASE_URL}/api/terminal`;

// Test configuration
export const options = {
  stages: SMOKE_TEST_STAGES,
  thresholds: {
    ws_connection_time: ['p(95)<100', 'p(99)<200'],
    ws_message_latency: [`p(95)<${THRESHOLDS.ws_latency_p95}`],
    ws_errors: ['rate<0.05'],
  },
  tags: {
    test_type: 'websocket',
  },
};

// Setup
export function setup() {
  console.log('=== WebSocket/Terminal Latency Test ===');
  console.log(`WebSocket URL: ${TERMINAL_WS_URL}`);
  return {
    startTime: Date.now(),
  };
}

// Main test function
export default function () {
  // Generate token for this connection
  const token = generateToken();

  // WebSocket URL with token query parameter (as per terminal route implementation)
  const url = `${TERMINAL_WS_URL}?token=${token}`;

  const connectionStartTime = Date.now();
  let connectionEstablished = false;
  let messagesExchanged = 0;

  // WebSocket connection with callbacks
  const response = ws.connect(url, {
    tags: { endpoint: 'terminal' },
  }, function (socket) {
    connectionEstablished = true;
    const connectionTime = Date.now() - connectionStartTime;
    wsConnectionTime.add(connectionTime);
    wsConnectionCount.add(1);

    // Connection opened handler
    socket.on('open', function () {
      check(connectionTime, {
        'connection established < 200ms': (t) => t < 200,
        'connection established < 500ms': (t) => t < 500,
      });

      // Send test messages to measure latency
      const messagesToSend = 5;
      let messageIndex = 0;

      // Send a ping-style message
      const sendMessage = () => {
        if (messageIndex >= messagesToSend) {
          socket.close();
          return;
        }

        const sendTime = Date.now();
        const message = JSON.stringify({
          type: 'ping',
          timestamp: sendTime,
          index: messageIndex,
        });

        socket.send(message);
        wsMessagesSent.add(1);
        messageIndex++;

        // Send next message after a short delay
        socket.setTimeout(() => sendMessage(), 500);
      };

      // Start sending messages after connection is stable
      socket.setTimeout(() => sendMessage(), 100);
    });

    // Message received handler
    socket.on('message', function (data) {
      const receiveTime = Date.now();
      wsMessagesReceived.add(1);
      messagesExchanged++;

      // Try to parse and measure latency
      try {
        // The server might echo back our message or send terminal output
        // For latency measurement, we use the receive time
        const message = JSON.parse(data);
        if (message.timestamp) {
          const latency = receiveTime - message.timestamp;
          wsMessageLatency.add(latency);

          check(latency, {
            'message latency < 50ms': (l) => l < 50,
            'message latency < 100ms': (l) => l < 100,
          });
        }
      } catch {
        // Binary or non-JSON message - just log receipt
        // Still counts as a successful message exchange
      }
    });

    // Error handler
    socket.on('error', function (e) {
      console.log(`WebSocket error: ${e.error()}`);
      wsErrors.add(1);
    });

    // Close handler
    socket.on('close', function () {
      // Connection closed normally
    });

    // Set timeout for the entire session
    socket.setTimeout(function () {
      socket.close();
    }, 10000); // 10 second max session
  });

  // Check connection result
  check(response, {
    'WebSocket connection successful': () => connectionEstablished,
    'Messages exchanged': () => messagesExchanged > 0,
  });

  if (!connectionEstablished) {
    wsErrors.add(1);
    console.log(`WebSocket connection failed (status: ${response?.status})`);
  }

  // Think time between WebSocket sessions
  sleep(2 + Math.random() * 2);
}

// Teardown
export function teardown(data) {
  const duration = (Date.now() - data.startTime) / 1000;
  console.log(`\nWebSocket test completed in ${duration.toFixed(2)} seconds`);
}

// Summary handler
export function handleSummary(data) {
  const summary = {
    timestamp: new Date().toISOString(),
    test: 'websocket-terminal',
    description: 'Tests PTY terminal WebSocket connections for latency',
    results: {
      totalConnections: data.metrics.ws_connections?.values?.count || 0,
      messagesSent: data.metrics.ws_messages_sent?.values?.count || 0,
      messagesReceived: data.metrics.ws_messages_received?.values?.count || 0,
    },
    connectionTime: {
      avg: data.metrics.ws_connection_time?.values?.avg?.toFixed(2) + 'ms',
      p95: data.metrics.ws_connection_time?.values?.['p(95)']?.toFixed(2) + 'ms',
      p99: data.metrics.ws_connection_time?.values?.['p(99)']?.toFixed(2) + 'ms',
    },
    messageLatency: {
      avg: data.metrics.ws_message_latency?.values?.avg?.toFixed(2) + 'ms',
      p95: data.metrics.ws_message_latency?.values?.['p(95)']?.toFixed(2) + 'ms',
      p99: data.metrics.ws_message_latency?.values?.['p(99)']?.toFixed(2) + 'ms',
    },
    errors: {
      errorRate: ((data.metrics.ws_errors?.values?.rate || 0) * 100).toFixed(2) + '%',
    },
    analysis: {
      connectionTimeOK: (data.metrics.ws_connection_time?.values?.['p(95)'] || 0) < 100,
      latencyOK: (data.metrics.ws_message_latency?.values?.['p(95)'] || Infinity) < THRESHOLDS.ws_latency_p95,
      errorRateOK: (data.metrics.ws_errors?.values?.rate || 0) < 0.05,
    },
  };

  // Console output
  console.log('\n' + '='.repeat(60));
  console.log('WEBSOCKET/TERMINAL TEST RESULTS');
  console.log('='.repeat(60));
  console.log(`Total Connections:    ${summary.results.totalConnections}`);
  console.log(`Messages Sent:        ${summary.results.messagesSent}`);
  console.log(`Messages Received:    ${summary.results.messagesReceived}`);
  console.log('-'.repeat(60));
  console.log('Connection Time:');
  console.log(`  Average:            ${summary.connectionTime.avg}`);
  console.log(`  p95:                ${summary.connectionTime.p95}`);
  console.log(`  p99:                ${summary.connectionTime.p99}`);
  console.log('-'.repeat(60));
  console.log('Message Latency:');
  console.log(`  Average:            ${summary.messageLatency.avg}`);
  console.log(`  p95:                ${summary.messageLatency.p95}`);
  console.log(`  p99:                ${summary.messageLatency.p99}`);
  console.log('-'.repeat(60));
  console.log('Analysis:');
  console.log(`  Connection Time OK: ${summary.analysis.connectionTimeOK ? 'YES' : 'NO'}`);
  console.log(`  Latency OK:         ${summary.analysis.latencyOK ? 'YES' : 'NO'}`);
  console.log(`  Error Rate OK:      ${summary.analysis.errorRateOK ? 'YES' : 'NO'}`);
  console.log('='.repeat(60));

  return {
    stdout: JSON.stringify(summary, null, 2),
    'results/websocket.json': JSON.stringify(summary, null, 2),
  };
}
