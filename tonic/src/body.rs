use crate::{Error, Status};
use bytes::{Buf, Bytes, IntoBuf};
use http_body::Body as HttpBody;
use std::pin::Pin;
use std::task::{Context, Poll};

pub type BytesBuf = <Bytes as IntoBuf>::Buf;

pub trait Body: sealed::Sealed {
    type Data: Buf;
    type Error: Into<Error>;

    fn is_end_stream(&self) -> bool;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>>;

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>>;
}

impl<T> Body for T
where
    T: HttpBody,
    T::Error: Into<Error>,
{
    type Data = T::Data;
    type Error = T::Error;

    fn is_end_stream(&self) -> bool {
        HttpBody::is_end_stream(self)
    }

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        HttpBody::poll_data(self, cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        HttpBody::poll_trailers(self, cx)
    }
}

impl<T> sealed::Sealed for T
where
    T: HttpBody,
    T::Error: Into<Error>,
{
}

mod sealed {
    pub trait Sealed {}
}

pub struct BoxBody {
    inner: Pin<Box<dyn Body<Data = BytesBuf, Error = Status> + Send + 'static>>,
}

struct MapBody<B>(B);

impl BoxBody {
    /// Create a new `BoxBody` mapping item and error to the default types.
    pub fn new<B>(inner: B) -> Self
    where
        B: Body<Data = BytesBuf, Error = Status> + Send + 'static,
    {
        BoxBody {
            inner: Box::pin(inner),
        }
    }

    /// Create a new `BoxBody` mapping item and error to the default types.
    pub fn map_from<B>(inner: B) -> Self
    where
        B: Body + Send + 'static,
        B::Data: Into<Bytes>,
        B::Error: Into<crate::Error>,
    {
        BoxBody {
            inner: Box::pin(MapBody(inner)),
        }
    }
}

impl HttpBody for BoxBody {
    type Data = BytesBuf;
    type Error = Status;

    fn is_end_stream(&self) -> bool {
        // Body::is_end_stream(&self.inner)
        self.inner.is_end_stream()
    }

    fn poll_data(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        Body::poll_data(self.inner.as_mut(), cx)
    }

    fn poll_trailers(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        Body::poll_trailers(self.inner.as_mut(), cx)
    }
}

impl<B> HttpBody for MapBody<B>
where
    B: Body,
    B::Data: Into<Bytes>,
    B::Error: Into<crate::Error>,
{
    type Data = BytesBuf;
    type Error = Status;

    fn is_end_stream(&self) -> bool {
        self.0.is_end_stream()
    }

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let v = unsafe {
            let me = self.get_unchecked_mut();
            Pin::new_unchecked(&mut me.0).poll_data(cx)
        };
        match futures_util::ready!(v) {
            Some(Ok(i)) => Poll::Ready(Some(Ok(i.into().into_buf()))),
            Some(Err(e)) => {
                let err = Status::map_error(e.into());
                Poll::Ready(Some(Err(err)))
            }
            None => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
        let v = unsafe {
            let me = self.get_unchecked_mut();
            Pin::new_unchecked(&mut me.0).poll_trailers(cx)
        };

        let v = futures_util::ready!(v).map_err(|e| Status::from_error(&*e.into()));
        Poll::Ready(v)
    }
}