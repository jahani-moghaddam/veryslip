use std::collections::VecDeque;
use std::sync::Arc;
use parking_lot::Mutex;
use std::time::Instant;
use tokio::sync::oneshot;

/// Priority queue for traffic scheduling
pub struct PriorityQueue {
    queues: [Arc<Mutex<VecDeque<PendingRequest>>>; 4],
    config: PriorityConfig,
    stats: Arc<Mutex<PriorityStats>>,
}

/// Priority configuration
#[derive(Debug, Clone)]
pub struct PriorityConfig {
    pub bandwidth_weights: [f32; 4],
    pub starvation_timeout: std::time::Duration,
    pub max_per_queue: usize,
}

impl Default for PriorityConfig {
    fn default() -> Self {
        Self {
            bandwidth_weights: [0.4, 0.3, 0.2, 0.1], // Critical, High, Medium, Low
            starvation_timeout: std::time::Duration::from_secs(30),
            max_per_queue: 1000,
        }
    }
}

/// Request priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Critical = 0,
    High = 1,
    Medium = 2,
    Low = 3,
}

impl Priority {
    pub fn from_usize(value: usize) -> Option<Self> {
        match value {
            0 => Some(Priority::Critical),
            1 => Some(Priority::High),
            2 => Some(Priority::Medium),
            3 => Some(Priority::Low),
            _ => None,
        }
    }

    /// Classify request by URL and headers
    pub fn classify(url: &str, _method: &str, headers: &[(String, String)]) -> Self {
        // Check for prefetch hint first
        for (key, value) in headers {
            if key.eq_ignore_ascii_case("purpose") && value == "prefetch" {
                return Priority::Low;
            }
        }

        // Critical: DNS, API, XHR
        if url.contains("/api/") || url.contains("/graphql") || url.ends_with(".json") {
            return Priority::Critical;
        }
        
        // Check for XHR/Fetch requests
        for (key, value) in headers {
            if key.eq_ignore_ascii_case("x-requested-with") && value == "XMLHttpRequest" {
                return Priority::Critical;
            }
            if key.eq_ignore_ascii_case("accept") && value.contains("application/json") {
                return Priority::Critical;
            }
        }

        // High: HTML, CSS, JS, fonts
        if url.ends_with(".html") || url.ends_with(".htm") || url.ends_with("/") {
            return Priority::High;
        }
        if url.ends_with(".css") || url.ends_with(".js") {
            return Priority::High;
        }
        if url.ends_with(".woff") || url.ends_with(".woff2") || url.ends_with(".ttf") {
            return Priority::High;
        }

        // Medium: Images
        if url.ends_with(".jpg") || url.ends_with(".jpeg") || url.ends_with(".png") 
            || url.ends_with(".webp") || url.ends_with(".gif") || url.ends_with(".svg") {
            return Priority::Medium;
        }

        // Low: Video, audio, large files
        if url.ends_with(".mp4") || url.ends_with(".webm") || url.ends_with(".mp3") 
            || url.ends_with(".ogg") || url.ends_with(".wav") {
            return Priority::Low;
        }

        // Default to High for unknown types
        Priority::High
    }
}

/// Pending request in queue
pub struct PendingRequest {
    pub url: String,
    pub method: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub priority: Priority,
    pub enqueued_at: Instant,
    pub response_tx: oneshot::Sender<HttpResponse>,
}

/// HTTP response
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Priority queue statistics
#[derive(Debug, Default, Clone)]
pub struct PriorityStats {
    pub enqueued: [u64; 4],
    pub dequeued: [u64; 4],
    pub elevated: u64,
}

impl PriorityQueue {
    /// Create new priority queue
    pub fn new(config: PriorityConfig) -> Self {
        Self {
            queues: [
                Arc::new(Mutex::new(VecDeque::new())),
                Arc::new(Mutex::new(VecDeque::new())),
                Arc::new(Mutex::new(VecDeque::new())),
                Arc::new(Mutex::new(VecDeque::new())),
            ],
            config,
            stats: Arc::new(Mutex::new(PriorityStats::default())),
        }
    }

    /// Enqueue a request
    pub async fn enqueue(&self, mut request: PendingRequest) -> crate::Result<()> {
        // Check for starvation and elevate if needed
        if request.priority == Priority::Low {
            let elapsed = request.enqueued_at.elapsed();
            if elapsed > self.config.starvation_timeout {
                request.priority = Priority::Medium;
                self.stats.lock().elevated += 1;
            }
        }

        let priority_idx = request.priority as usize;
        let mut queue = self.queues[priority_idx].lock();
        
        if queue.len() >= self.config.max_per_queue {
            return Err(crate::VerySlipError::QueueFull);
        }

        queue.push_back(request);
        self.stats.lock().enqueued[priority_idx] += 1;
        
        Ok(())
    }

    /// Dequeue next request using weighted round-robin
    pub async fn dequeue(&self) -> Option<PendingRequest> {
        // Try weighted selection up to 4 times
        for _ in 0..4 {
            let rand_val = rand::random::<f32>();
            let mut cumulative = 0.0;
            
            for (idx, &weight) in self.config.bandwidth_weights.iter().enumerate() {
                cumulative += weight;
                if rand_val < cumulative {
                    let mut queue = self.queues[idx].lock();
                    if let Some(request) = queue.pop_front() {
                        self.stats.lock().dequeued[idx] += 1;
                        return Some(request);
                    }
                    break; // Queue was empty, try again with new random
                }
            }
        }

        // Final fallback: try all queues in priority order
        for idx in 0..4 {
            let mut queue = self.queues[idx].lock();
            if let Some(request) = queue.pop_front() {
                self.stats.lock().dequeued[idx] += 1;
                return Some(request);
            }
        }

        None
    }

    /// Get queue statistics
    pub fn stats(&self) -> PriorityStats {
        self.stats.lock().clone()
    }

    /// Get queue lengths
    pub fn queue_lengths(&self) -> [usize; 4] {
        [
            self.queues[0].lock().len(),
            self.queues[1].lock().len(),
            self.queues[2].lock().len(),
            self.queues[3].lock().len(),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_priority_classification() {
        // Critical: API
        assert_eq!(Priority::classify("https://example.com/api/users", "GET", &[]), Priority::Critical);
        assert_eq!(Priority::classify("https://example.com/graphql", "POST", &[]), Priority::Critical);
        assert_eq!(Priority::classify("https://example.com/data.json", "GET", &[]), Priority::Critical);

        // High: HTML, CSS, JS
        assert_eq!(Priority::classify("https://example.com/index.html", "GET", &[]), Priority::High);
        assert_eq!(Priority::classify("https://example.com/style.css", "GET", &[]), Priority::High);
        assert_eq!(Priority::classify("https://example.com/app.js", "GET", &[]), Priority::High);

        // Medium: Images
        assert_eq!(Priority::classify("https://example.com/image.jpg", "GET", &[]), Priority::Medium);
        assert_eq!(Priority::classify("https://example.com/photo.png", "GET", &[]), Priority::Medium);

        // Low: Video
        assert_eq!(Priority::classify("https://example.com/video.mp4", "GET", &[]), Priority::Low);
    }

    #[tokio::test]
    async fn test_xhr_classification() {
        let headers = vec![
            ("X-Requested-With".to_string(), "XMLHttpRequest".to_string()),
        ];
        assert_eq!(Priority::classify("https://example.com/data", "GET", &headers), Priority::Critical);
    }

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let config = PriorityConfig::default();
        let queue = PriorityQueue::new(config);

        let (tx, _rx) = oneshot::channel();
        let request = PendingRequest {
            url: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
            body: vec![],
            priority: Priority::High,
            enqueued_at: Instant::now(),
            response_tx: tx,
        };

        queue.enqueue(request).await.unwrap();
        
        let lengths = queue.queue_lengths();
        assert_eq!(lengths[Priority::High as usize], 1);

        let dequeued = queue.dequeue().await;
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().url, "https://example.com");
    }

    #[tokio::test]
    async fn test_queue_limit() {
        let config = PriorityConfig {
            max_per_queue: 2,
            ..Default::default()
        };
        let queue = PriorityQueue::new(config);

        for i in 0..2 {
            let (tx, _rx) = oneshot::channel();
            let request = PendingRequest {
                url: format!("https://example.com/{}", i),
                method: "GET".to_string(),
                headers: vec![],
                body: vec![],
                priority: Priority::High,
                enqueued_at: Instant::now(),
                response_tx: tx,
            };
            queue.enqueue(request).await.unwrap();
        }

        // Third request should fail
        let (tx, _rx) = oneshot::channel();
        let request = PendingRequest {
            url: "https://example.com/3".to_string(),
            method: "GET".to_string(),
            headers: vec![],
            body: vec![],
            priority: Priority::High,
            enqueued_at: Instant::now(),
            response_tx: tx,
        };
        assert!(queue.enqueue(request).await.is_err());
    }

    #[tokio::test]
    async fn test_starvation_prevention() {
        let config = PriorityConfig {
            starvation_timeout: std::time::Duration::from_millis(10),
            ..Default::default()
        };
        let queue = PriorityQueue::new(config);

        // Enqueue low priority request
        let (tx, _rx) = oneshot::channel();
        let request = PendingRequest {
            url: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
            body: vec![],
            priority: Priority::Low,
            enqueued_at: Instant::now() - std::time::Duration::from_secs(31),
            response_tx: tx,
        };

        queue.enqueue(request).await.unwrap();

        // Should be elevated to Medium
        let lengths = queue.queue_lengths();
        assert_eq!(lengths[Priority::Medium as usize], 1);
        assert_eq!(lengths[Priority::Low as usize], 0);

        let stats = queue.stats();
        assert_eq!(stats.elevated, 1);
    }

    #[tokio::test]
    async fn test_weighted_scheduling() {
        let config = PriorityConfig::default();
        let queue = PriorityQueue::new(config);

        // Enqueue requests at all priority levels
        for _ in 0..40 {
            let (tx, _rx) = oneshot::channel();
            queue.enqueue(PendingRequest {
                url: "https://example.com/critical".to_string(),
                method: "GET".to_string(),
                headers: vec![],
                body: vec![],
                priority: Priority::Critical,
                enqueued_at: Instant::now(),
                response_tx: tx,
            }).await.unwrap();
        }

        for _ in 0..30 {
            let (tx, _rx) = oneshot::channel();
            queue.enqueue(PendingRequest {
                url: "https://example.com/high".to_string(),
                method: "GET".to_string(),
                headers: vec![],
                body: vec![],
                priority: Priority::High,
                enqueued_at: Instant::now(),
                response_tx: tx,
            }).await.unwrap();
        }

        for _ in 0..20 {
            let (tx, _rx) = oneshot::channel();
            queue.enqueue(PendingRequest {
                url: "https://example.com/medium".to_string(),
                method: "GET".to_string(),
                headers: vec![],
                body: vec![],
                priority: Priority::Medium,
                enqueued_at: Instant::now(),
                response_tx: tx,
            }).await.unwrap();
        }

        for _ in 0..10 {
            let (tx, _rx) = oneshot::channel();
            queue.enqueue(PendingRequest {
                url: "https://example.com/low".to_string(),
                method: "GET".to_string(),
                headers: vec![],
                body: vec![],
                priority: Priority::Low,
                enqueued_at: Instant::now(),
                response_tx: tx,
            }).await.unwrap();
        }

        // Dequeue and count
        let mut critical_count = 0;
        let mut high_count = 0;
        let mut medium_count = 0;
        let mut low_count = 0;

        for _ in 0..100 {
            if let Some(req) = queue.dequeue().await {
                if req.url.contains("critical") {
                    critical_count += 1;
                } else if req.url.contains("high") {
                    high_count += 1;
                } else if req.url.contains("medium") {
                    medium_count += 1;
                } else {
                    low_count += 1;
                }
            }
        }

        // Verify all requests were dequeued
        assert_eq!(critical_count, 40);
        assert_eq!(high_count, 30);
        assert_eq!(medium_count, 20);
        assert_eq!(low_count, 10);
    }

    #[tokio::test]
    async fn test_stats() {
        let config = PriorityConfig::default();
        let queue = PriorityQueue::new(config);

        let (tx, _rx) = oneshot::channel();
        queue.enqueue(PendingRequest {
            url: "https://example.com".to_string(),
            method: "GET".to_string(),
            headers: vec![],
            body: vec![],
            priority: Priority::High,
            enqueued_at: Instant::now(),
            response_tx: tx,
        }).await.unwrap();

        let stats = queue.stats();
        assert_eq!(stats.enqueued[Priority::High as usize], 1);

        queue.dequeue().await;

        let stats = queue.stats();
        assert_eq!(stats.dequeued[Priority::High as usize], 1);
    }

    #[tokio::test]
    async fn test_empty_dequeue() {
        let config = PriorityConfig::default();
        let queue = PriorityQueue::new(config);

        let result = queue.dequeue().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_prefetch_priority() {
        let headers = vec![
            ("Purpose".to_string(), "prefetch".to_string()),
        ];
        assert_eq!(Priority::classify("https://example.com/image.jpg", "GET", &headers), Priority::Low);
    }
}
