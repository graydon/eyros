use eyros::{DB,Setup,Tree2,QTrace,TreeRef,Row,Coord,Error};
use random::{Source,default as rand};
use tempfile::Builder as Tmpfile;
use async_std::{prelude::*,channel};
use std::collections::HashMap;
use desert::CountBytes;

type P = (Coord<f32>,Coord<f32>);
type V = Vec<u8>;
type T = Tree2<f32,f32,V>;

#[async_std::test]
async fn max_tree_bytes_1() -> Result<(),Error> {
  let dir = Tmpfile::new().prefix("eyros").tempdir()?;
  let max_tree_bytes = 5_000;
  let mut db: DB<_,T,P,V> = Setup::from_path(dir.path())
    .max_tree_bytes(max_tree_bytes)
    .build().await?
  ;
  db.batch(&vec![
    Row::Insert((Coord::Scalar(1.0),Coord::Scalar(3.0)),vec![1;1_000]),
    Row::Insert((Coord::Scalar(1.0),Coord::Scalar(2.0)),vec![2;2_000]),
    Row::Insert((Coord::Interval(6.0,9.0),Coord::Interval(4.0,5.0)),vec![3;1_000]),
    Row::Insert((Coord::Interval(-2.5,0.5),Coord::Scalar(-1.2)),vec![4;2_000]),
    Row::Insert((Coord::Scalar(-4.5),Coord::Interval(-5.5,-1.2)),vec![5;1_000]),
    Row::Insert((Coord::Interval(-9.0,-8.0),Coord::Interval(-4.0,4.0)),vec![6;1_000]),
  ]).await?;
  db.sync().await?;

  let trace = Box::new(Trace::default());
  let bbox = ((-10.0,-10.0),(10.0,10.0));
  let mut stream = db.query_trace(&bbox, trace.clone()).await?;
  let mut count = 0;
  while let Some(result) = stream.next().await {
    result?;
    count += 1;
  }
  trace.close();
  assert_eq![count, 6];

  let mut sizes = HashMap::new();
  while let Ok(r) = trace.next().await {
    let t = db.trees.get(&r.id).await?;
    let bytes = t.lock().await.count_bytes();
    assert![bytes <= max_tree_bytes, "{} <= {}", bytes, max_tree_bytes];
    sizes.insert(r.id, bytes);
  }

  Ok(())
}

#[async_std::test]
async fn max_tree_bytes_2() -> Result<(),Error> {
  let dir = Tmpfile::new().prefix("eyros").tempdir()?;
  let max_tree_bytes = 250_000;
  let mut db: DB<_,T,P,V> = Setup::from_path(dir.path())
    .max_tree_bytes(max_tree_bytes)
    .build().await?
  ;
  let mut r = rand().seed([13,12]);
  let (batch_size,nbatches) = (1_000,10);
  for _ in 0..nbatches {
    let batch: Vec<Row<P,V>> = (0..batch_size).map(|_| {
      let (point,value) = {
        let n = (r.read::<f32>()*500.0+1.0) as usize;
        let buf = r.iter().take(n).collect::<Vec<u8>>();
        if r.read::<f32>() > 0.5 {
          let xmin: f32 = r.read::<f32>()*2.0-1.0;
          let xmax: f32 = xmin + r.read::<f32>().powf(2.0)*(1.0-xmin);
          let ymin: f32 = r.read::<f32>()*2.0-1.0;
          let ymax: f32 = ymin + r.read::<f32>().powf(2.0)*(1.0-ymin);
          (
            (Coord::Interval(xmin,xmax),Coord::Interval(ymin,ymax)),
            buf
          )
        } else {
          let x: f32 = r.read::<f32>()*2.0-1.0;
          let y: f32 = r.read::<f32>()*2.0-1.0;
          (
            (Coord::Scalar(x),Coord::Scalar(y)),
            buf
          )
        }
      };
      Row::Insert(point,value)
    }).collect();
    db.batch(&batch).await?;
  }
  db.sync().await?;

  let trace = Box::new(Trace::default());
  let bbox = ((-10.0,-10.0),(10.0,10.0));
  let mut stream = db.query_trace(&bbox, trace.clone()).await?;
  let mut count = 0;
  while let Some(result) = stream.next().await {
    result?;
    count += 1;
  }
  trace.close();
  assert_eq![count, batch_size * nbatches];

  let mut sizes = HashMap::new();
  while let Ok(r) = trace.next().await {
    let t = db.trees.get(&r.id).await?;
    let bytes = t.lock().await.count_bytes();
    assert![bytes <= max_tree_bytes, "{} <= {}", bytes, max_tree_bytes];
    sizes.insert(r.id, bytes);
  }

  Ok(())
}


struct Trace {
  receiver: channel::Receiver<TreeRef<P>>,
  sender: channel::Sender<TreeRef<P>>,
}

impl Clone for Trace {
  fn clone(&self) -> Self {
    Self {
      receiver: self.receiver.clone(),
      sender: self.sender.clone(),
    }
  }
}

impl Default for Trace {
  fn default() -> Self {
    let (sender,receiver) = channel::unbounded();
    Self { sender, receiver }
  }
}

impl Trace {
  async fn next(&self) -> Result<TreeRef<P>,Error> {
    self.receiver.recv().await.map_err(|e| e.into())
  }
  fn close(&self) {
    self.sender.close();
  }
}

impl QTrace<P> for Trace {
  fn trace(&mut self, tr: TreeRef<P>) {
    let ch = self.sender.clone();
    ch.try_send(tr).unwrap();
  }
}
