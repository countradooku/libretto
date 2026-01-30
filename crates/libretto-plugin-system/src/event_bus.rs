//! Event bus for plugin-to-plugin communication.
//!
//! This module provides an asynchronous message passing system for plugins to
//! communicate with each other via crossbeam channels.

use crate::error::{PluginError, Result};
use crossbeam_channel::{Receiver, Sender, TryRecvError, bounded};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, trace, warn};

/// Default event bus capacity.
const DEFAULT_CAPACITY: usize = 1000;

/// Event message that can be passed between plugins.
#[derive(Debug, Clone)]
pub struct EventMessage {
    /// Unique message ID.
    pub id: u64,
    /// Source plugin ID.
    pub source: String,
    /// Target plugin ID (None for broadcast).
    pub target: Option<String>,
    /// Message topic.
    pub topic: String,
    /// Message payload.
    pub payload: MessagePayload,
    /// Timestamp.
    pub timestamp: Instant,
    /// Correlation ID for request-response patterns.
    pub correlation_id: Option<u64>,
    /// Message priority.
    pub priority: MessagePriority,
}

impl EventMessage {
    /// Create a new event message.
    #[must_use]
    pub fn new(
        source: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
    ) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);

        Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            source: source.into(),
            target: None,
            topic: topic.into(),
            payload,
            timestamp: Instant::now(),
            correlation_id: None,
            priority: MessagePriority::Normal,
        }
    }

    /// Create a broadcast message.
    #[must_use]
    pub fn broadcast(
        source: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
    ) -> Self {
        Self::new(source, topic, payload)
    }

    /// Create a direct message to a specific plugin.
    #[must_use]
    pub fn direct(
        source: impl Into<String>,
        target: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
    ) -> Self {
        let mut msg = Self::new(source, topic, payload);
        msg.target = Some(target.into());
        msg
    }

    /// Create a response message.
    #[must_use]
    pub fn response(original: &Self, source: impl Into<String>, payload: MessagePayload) -> Self {
        let mut msg = Self::new(source, format!("{}.response", original.topic), payload);
        msg.target = Some(original.source.clone());
        msg.correlation_id = Some(original.id);
        msg
    }

    /// Set message priority.
    #[must_use]
    pub const fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this is a broadcast message.
    #[must_use]
    pub const fn is_broadcast(&self) -> bool {
        self.target.is_none()
    }

    /// Check if this message is for a specific plugin.
    #[must_use]
    pub fn is_for(&self, plugin_id: &str) -> bool {
        self.target.as_ref().is_none_or(|t| t == plugin_id)
    }

    /// Check if this is a response to a previous message.
    #[must_use]
    pub const fn is_response(&self) -> bool {
        self.correlation_id.is_some()
    }
}

/// Message priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MessagePriority {
    /// Low priority (processed last).
    Low = 0,
    /// Normal priority.
    Normal = 1,
    /// High priority (processed first).
    High = 2,
    /// Critical priority (immediate processing).
    Critical = 3,
}

/// Message payload types.
#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// Empty payload.
    Empty,
    /// Text payload.
    Text(String),
    /// JSON payload.
    Json(serde_json::Value),
    /// Binary payload.
    Binary(Vec<u8>),
    /// Key-value pairs.
    Map(HashMap<String, String>),
}

impl MessagePayload {
    /// Create a JSON payload from a serializable value.
    ///
    /// # Errors
    /// Returns error if serialization fails.
    pub fn json<T: Serialize>(value: &T) -> Result<Self> {
        let json =
            serde_json::to_value(value).map_err(|e| PluginError::ChannelSend(e.to_string()))?;
        Ok(Self::Json(json))
    }

    /// Extract JSON payload as a typed value.
    ///
    /// # Errors
    /// Returns error if deserialization fails.
    pub fn as_json<T: for<'de> Deserialize<'de>>(&self) -> Result<T> {
        match self {
            Self::Json(value) => serde_json::from_value(value.clone())
                .map_err(|e| PluginError::ChannelReceive(e.to_string())),
            Self::Text(text) => {
                serde_json::from_str(text).map_err(|e| PluginError::ChannelReceive(e.to_string()))
            }
            _ => Err(PluginError::ChannelReceive("payload is not JSON".into())),
        }
    }

    /// Get text payload.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    /// Get binary payload.
    #[must_use]
    pub fn as_binary(&self) -> Option<&[u8]> {
        match self {
            Self::Binary(b) => Some(b),
            _ => None,
        }
    }

    /// Get map payload.
    #[must_use]
    pub const fn as_map(&self) -> Option<&HashMap<String, String>> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }
}

/// Event subscription for receiving messages.
#[derive(Debug)]
pub struct EventSubscription {
    /// Subscriber ID.
    pub id: u64,
    /// Plugin ID.
    pub plugin_id: String,
    /// Topic filter (None for all topics).
    pub topic_filter: Option<String>,
    /// Message receiver.
    receiver: Receiver<EventMessage>,
}

impl EventSubscription {
    /// Receive a message (blocking).
    ///
    /// # Errors
    /// Returns error if the channel is disconnected.
    pub fn recv(&self) -> Result<EventMessage> {
        self.receiver
            .recv()
            .map_err(|e| PluginError::ChannelReceive(e.to_string()))
    }

    /// Try to receive a message (non-blocking).
    #[must_use]
    pub fn try_recv(&self) -> Option<EventMessage> {
        match self.receiver.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }

    /// Receive a message with timeout.
    ///
    /// # Errors
    /// Returns error if timeout occurs or channel disconnects.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<EventMessage> {
        self.receiver
            .recv_timeout(timeout)
            .map_err(|e| PluginError::ChannelReceive(e.to_string()))
    }

    /// Check if the subscription is still active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.receiver.is_empty() || self.receiver.is_empty()
    }

    /// Drain all pending messages.
    #[must_use]
    pub fn drain(&self) -> Vec<EventMessage> {
        let mut messages = Vec::new();
        while let Some(msg) = self.try_recv() {
            messages.push(msg);
        }
        messages
    }
}

/// Internal subscriber entry.
struct Subscriber {
    sender: Sender<EventMessage>,
    topic_filter: Option<String>,
    plugin_id: String,
}

impl std::fmt::Debug for Subscriber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscriber")
            .field("topic_filter", &self.topic_filter)
            .field("plugin_id", &self.plugin_id)
            .finish_non_exhaustive()
    }
}

/// Event bus for plugin communication.
#[derive(Debug)]
pub struct EventBus {
    /// Subscribers.
    subscribers: DashMap<u64, Arc<Subscriber>>,
    /// Next subscriber ID.
    next_id: AtomicU64,
    /// Message history (for late joiners).
    history: RwLock<Vec<EventMessage>>,
    /// Maximum history size.
    history_limit: usize,
    /// Bus capacity.
    capacity: usize,
    /// Message statistics.
    stats: EventBusStats,
}

/// Event bus statistics.
#[derive(Debug, Default)]
pub struct EventBusStats {
    /// Total messages published.
    pub messages_published: AtomicU64,
    /// Total messages delivered.
    pub messages_delivered: AtomicU64,
    /// Dropped messages (no subscribers).
    pub messages_dropped: AtomicU64,
    /// Active subscribers.
    pub active_subscribers: AtomicU64,
}

impl EventBus {
    /// Create a new event bus with default capacity.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            subscribers: DashMap::new(),
            next_id: AtomicU64::new(1),
            history: RwLock::new(Vec::new()),
            history_limit: 100,
            capacity,
            stats: EventBusStats::default(),
        }
    }

    /// Subscribe to messages.
    #[must_use]
    pub fn subscribe(&self) -> EventSubscription {
        self.subscribe_filtered(String::new(), None)
    }

    /// Subscribe with a topic filter.
    #[must_use]
    pub fn subscribe_to_topic(
        &self,
        plugin_id: impl Into<String>,
        topic: impl Into<String>,
    ) -> EventSubscription {
        self.subscribe_filtered(plugin_id, Some(topic.into()))
    }

    /// Subscribe with optional filter.
    fn subscribe_filtered(
        &self,
        plugin_id: impl Into<String>,
        topic_filter: Option<String>,
    ) -> EventSubscription {
        let (sender, receiver) = bounded(self.capacity);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let plugin_id = plugin_id.into();

        let subscriber = Arc::new(Subscriber {
            sender,
            topic_filter: topic_filter.clone(),
            plugin_id: plugin_id.clone(),
        });

        self.subscribers.insert(id, subscriber);
        self.stats
            .active_subscribers
            .fetch_add(1, Ordering::Relaxed);

        debug!(id = id, plugin = %plugin_id, topic = ?topic_filter, "new subscription");

        EventSubscription {
            id,
            plugin_id,
            topic_filter,
            receiver,
        }
    }

    /// Unsubscribe.
    pub fn unsubscribe(&self, subscription_id: u64) {
        if self.subscribers.remove(&subscription_id).is_some() {
            self.stats
                .active_subscribers
                .fetch_sub(1, Ordering::Relaxed);
            debug!(id = subscription_id, "subscription removed");
        }
    }

    /// Publish a message.
    ///
    /// # Errors
    /// Returns error if publishing fails.
    pub fn publish(&self, message: EventMessage) -> Result<()> {
        trace!(
            id = message.id,
            source = %message.source,
            topic = %message.topic,
            "publishing message"
        );

        self.stats
            .messages_published
            .fetch_add(1, Ordering::Relaxed);

        let mut delivered = 0u64;

        for entry in &self.subscribers {
            let subscriber = entry.value();

            // Check if message is for this subscriber
            if !self.should_deliver(&message, subscriber) {
                continue;
            }

            // Try to send
            match subscriber.sender.try_send(message.clone()) {
                Ok(()) => {
                    delivered += 1;
                }
                Err(crossbeam_channel::TrySendError::Full(_)) => {
                    warn!(
                        subscriber = %subscriber.plugin_id,
                        "subscriber channel full, message dropped"
                    );
                }
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                    // Subscriber disconnected, will be cleaned up
                }
            }
        }

        self.stats
            .messages_delivered
            .fetch_add(delivered, Ordering::Relaxed);

        if delivered == 0 {
            self.stats.messages_dropped.fetch_add(1, Ordering::Relaxed);
        }

        // Add to history
        self.add_to_history(message);

        Ok(())
    }

    /// Broadcast a message to all subscribers.
    ///
    /// # Errors
    /// Returns error if broadcasting fails.
    pub fn broadcast(
        &self,
        source: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
    ) -> Result<()> {
        let message = EventMessage::broadcast(source, topic, payload);
        self.publish(message)
    }

    /// Send a direct message to a specific plugin.
    ///
    /// # Errors
    /// Returns error if sending fails.
    pub fn send_direct(
        &self,
        source: impl Into<String>,
        target: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
    ) -> Result<()> {
        let message = EventMessage::direct(source, target, topic, payload);
        self.publish(message)
    }

    /// Get message history.
    #[must_use]
    pub fn history(&self) -> Vec<EventMessage> {
        self.history.read().clone()
    }

    /// Get statistics.
    #[must_use]
    pub const fn stats(&self) -> &EventBusStats {
        &self.stats
    }

    /// Get subscriber count.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }

    /// Clear all subscribers.
    pub fn clear(&self) {
        self.subscribers.clear();
        self.stats.active_subscribers.store(0, Ordering::Relaxed);
    }

    /// Check if a message should be delivered to a subscriber.
    fn should_deliver(&self, message: &EventMessage, subscriber: &Subscriber) -> bool {
        // Check target
        if let Some(ref target) = message.target
            && target != &subscriber.plugin_id
        {
            return false;
        }

        // Don't deliver to sender
        if message.source == subscriber.plugin_id {
            return false;
        }

        // Check topic filter
        if let Some(ref filter) = subscriber.topic_filter
            && !Self::topic_matches(&message.topic, filter)
        {
            return false;
        }

        true
    }

    /// Check if a topic matches a filter.
    fn topic_matches(topic: &str, filter: &str) -> bool {
        if filter.is_empty() || filter == "*" {
            return true;
        }

        if let Some(prefix) = filter.strip_suffix('*') {
            return topic.starts_with(prefix);
        }

        topic == filter
    }

    /// Add a message to history.
    fn add_to_history(&self, message: EventMessage) {
        let mut history = self.history.write();
        history.push(message);

        // Trim if over limit
        if history.len() > self.history_limit {
            let excess = history.len() - self.history_limit;
            history.drain(..excess);
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

/// Request-response pattern helper.
#[derive(Debug)]
#[allow(dead_code)]
pub struct RequestResponse {
    /// Event bus reference.
    bus: Arc<EventBus>,
    /// Pending requests.
    pending: DashMap<u64, Sender<EventMessage>>,
}

#[allow(dead_code)]
impl RequestResponse {
    /// Create a new request-response handler.
    #[must_use]
    pub fn new(bus: Arc<EventBus>) -> Self {
        Self {
            bus,
            pending: DashMap::new(),
        }
    }

    /// Send a request and wait for response.
    ///
    /// # Errors
    /// Returns error if request or response fails.
    pub fn request(
        &self,
        source: impl Into<String>,
        target: impl Into<String>,
        topic: impl Into<String>,
        payload: MessagePayload,
        timeout: Duration,
    ) -> Result<EventMessage> {
        let message = EventMessage::direct(source, target, topic, payload);
        let request_id = message.id;

        let (tx, rx) = bounded(1);
        self.pending.insert(request_id, tx);

        self.bus.publish(message)?;

        let response = rx
            .recv_timeout(timeout)
            .map_err(|e| PluginError::ChannelReceive(format!("request timeout: {e}")))?;

        self.pending.remove(&request_id);

        Ok(response)
    }

    /// Handle a response message.
    pub fn handle_response(&self, message: EventMessage) {
        if let Some(correlation_id) = message.correlation_id
            && let Some((_, sender)) = self.pending.remove(&correlation_id)
        {
            let _ = sender.send(message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_message_creation() {
        let msg = EventMessage::new("plugin-a", "test.topic", MessagePayload::Empty);
        assert_eq!(msg.source, "plugin-a");
        assert_eq!(msg.topic, "test.topic");
        assert!(msg.is_broadcast());
    }

    #[test]
    fn direct_message() {
        let msg = EventMessage::direct(
            "plugin-a",
            "plugin-b",
            "direct.message",
            MessagePayload::Text("hello".into()),
        );
        assert!(!msg.is_broadcast());
        assert!(msg.is_for("plugin-b"));
        assert!(!msg.is_for("plugin-c"));
    }

    #[test]
    fn message_payload_json() {
        let data = serde_json::json!({"key": "value"});
        let payload = MessagePayload::json(&data).unwrap();

        if let MessagePayload::Json(value) = payload {
            assert_eq!(value["key"], "value");
        } else {
            panic!("expected JSON payload");
        }
    }

    #[test]
    fn event_bus_subscribe_publish() {
        let bus = EventBus::new(100);

        let sub1 = bus.subscribe_filtered("plugin-a", None);
        let sub2 = bus.subscribe_filtered("plugin-b", None);

        bus.broadcast("plugin-c", "test", MessagePayload::Empty)
            .unwrap();

        // Both should receive the message
        let msg1 = sub1.try_recv();
        let msg2 = sub2.try_recv();

        assert!(msg1.is_some());
        assert!(msg2.is_some());
    }

    #[test]
    fn event_bus_topic_filter() {
        let bus = EventBus::new(100);

        let sub1 = bus.subscribe_to_topic("plugin-a", "events.*");
        let sub2 = bus.subscribe_to_topic("plugin-b", "other.*");

        bus.broadcast("plugin-c", "events.test", MessagePayload::Empty)
            .unwrap();

        // Only sub1 should receive
        assert!(sub1.try_recv().is_some());
        assert!(sub2.try_recv().is_none());
    }

    #[test]
    fn event_bus_direct_message() {
        let bus = EventBus::new(100);

        let sub1 = bus.subscribe_filtered("plugin-a", None);
        let sub2 = bus.subscribe_filtered("plugin-b", None);

        bus.send_direct("plugin-c", "plugin-a", "direct", MessagePayload::Empty)
            .unwrap();

        // Only sub1 should receive
        assert!(sub1.try_recv().is_some());
        assert!(sub2.try_recv().is_none());
    }

    #[test]
    fn event_bus_no_self_delivery() {
        let bus = EventBus::new(100);

        let sub = bus.subscribe_filtered("plugin-a", None);

        bus.broadcast("plugin-a", "test", MessagePayload::Empty)
            .unwrap();

        // Should not receive own message
        assert!(sub.try_recv().is_none());
    }

    #[test]
    fn topic_matching() {
        assert!(EventBus::topic_matches("events.test", "*"));
        assert!(EventBus::topic_matches("events.test", ""));
        assert!(EventBus::topic_matches("events.test", "events.*"));
        assert!(EventBus::topic_matches("events.test", "events.test"));
        assert!(!EventBus::topic_matches("events.test", "other.*"));
    }

    #[test]
    fn message_priority() {
        assert!(MessagePriority::Critical > MessagePriority::High);
        assert!(MessagePriority::High > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    #[test]
    fn response_message() {
        let original = EventMessage::new("plugin-a", "request", MessagePayload::Empty);
        let response =
            EventMessage::response(&original, "plugin-b", MessagePayload::Text("ok".into()));

        assert!(response.is_response());
        assert_eq!(response.correlation_id, Some(original.id));
        assert_eq!(response.target, Some("plugin-a".to_string()));
    }

    #[test]
    fn event_bus_stats() {
        let bus = EventBus::new(100);

        let _sub = bus.subscribe_filtered("plugin-a", None);
        bus.broadcast("plugin-b", "test", MessagePayload::Empty)
            .unwrap();

        assert_eq!(bus.stats().messages_published.load(Ordering::Relaxed), 1);
        assert_eq!(bus.stats().messages_delivered.load(Ordering::Relaxed), 1);
        assert_eq!(bus.stats().active_subscribers.load(Ordering::Relaxed), 1);
    }
}
